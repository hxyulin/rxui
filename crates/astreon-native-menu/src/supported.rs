use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

use astrelis_platform::Window;
use astreon_app::{CommandId, CommandRegistry, Menu, MenuBar, MenuEntry, MenuRole, Shortcut};
use muda::{
    CheckMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
    accelerator::{Key, KeyAccelerator, Modifiers},
};

use crate::{NativeMenuError, NativeMenuEvent};

static INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
enum NativeCommandItem {
    Normal(MenuItem),
    Check(CheckMenuItem),
}

impl NativeCommandItem {
    fn sync<Message>(
        &self,
        command: &astreon_app::Command<Message>,
    ) -> Result<(), NativeMenuError> {
        match self {
            Self::Normal(item) => {
                item.set_text(&command.label);
                item.set_enabled(command.enabled);
                item.set_key_accelerator(key_accelerator(command.shortcut.as_ref()))
                    .map_err(NativeMenuError::from_display)?;
            }
            Self::Check(item) => {
                item.set_text(&command.label);
                item.set_enabled(command.enabled);
                item.set_checked(command.checked.unwrap_or(false));
                item.set_key_accelerator(key_accelerator(command.shortcut.as_ref()))
                    .map_err(NativeMenuError::from_display)?;
            }
        }
        Ok(())
    }
}

pub(crate) struct ApplicationMenu {
    native: muda::Menu,
    command_ids: BTreeMap<String, CommandId>,
    items: BTreeMap<CommandId, Vec<NativeCommandItem>>,
    windows: Vec<Window>,
}

impl ApplicationMenu {
    pub(crate) fn install<Message, F>(
        window: &Window,
        model: MenuBar,
        commands: &CommandRegistry<Message>,
        wake: F,
    ) -> Result<Self, NativeMenuError>
    where
        F: Fn(NativeMenuEvent) + Send + Sync + 'static,
    {
        if INSTALLED.swap(true, Ordering::AcqRel) {
            return Err(NativeMenuError::from_display(
                "an application menu is already installed",
            ));
        }
        let result = Self::build(window, model, commands, wake);
        if result.is_err() {
            INSTALLED.store(false, Ordering::Release);
        }
        result
    }

    fn build<Message, F>(
        window: &Window,
        model: MenuBar,
        commands: &CommandRegistry<Message>,
        wake: F,
    ) -> Result<Self, NativeMenuError>
    where
        F: Fn(NativeMenuEvent) + Send + Sync + 'static,
    {
        validate_roles(&model)?;
        let native = muda::Menu::new();
        let mut command_ids = BTreeMap::new();
        let mut items = BTreeMap::new();
        let mut next_id = 0_u64;
        for menu in &model.menus {
            let submenu = build_menu(menu, commands, &mut command_ids, &mut items, &mut next_id)?;
            native
                .append(&submenu)
                .map_err(NativeMenuError::from_display)?;
        }
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            wake(NativeMenuEvent {
                id: event.id().as_ref().to_owned(),
            });
        }));
        let mut result = Self {
            native,
            command_ids,
            items,
            windows: Vec::new(),
        };
        result.install_native(window)?;
        result.windows.push(window.clone());
        Ok(result)
    }

    pub(crate) fn attach_window(&mut self, window: &Window) -> Result<(), NativeMenuError> {
        if self
            .windows
            .iter()
            .any(|current| current.id() == window.id())
        {
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        self.install_native(window)?;
        self.windows.push(window.clone());
        Ok(())
    }

    pub(crate) fn detach_window(&mut self, window: &Window) -> Result<(), NativeMenuError> {
        if !self
            .windows
            .iter()
            .any(|current| current.id() == window.id())
        {
            return Ok(());
        }
        self.remove_native(window)?;
        self.windows.retain(|current| current.id() != window.id());
        Ok(())
    }

    pub(crate) fn sync<Message>(
        &mut self,
        commands: &CommandRegistry<Message>,
    ) -> Result<(), NativeMenuError> {
        for (id, native_items) in &self.items {
            let command = commands.get(id).ok_or_else(|| {
                NativeMenuError::from_display(format!(
                    "installed menu references removed command `{id}`"
                ))
            })?;
            for item in native_items {
                item.sync(command)?;
            }
        }
        Ok(())
    }

    pub(crate) fn dispatch<Message: Clone>(
        &self,
        event: &NativeMenuEvent,
        commands: &CommandRegistry<Message>,
    ) -> Option<Message> {
        self.command_ids
            .get(&event.id)
            .and_then(|id| commands.invoke(id))
    }

    #[cfg(target_os = "macos")]
    fn install_native(&self, _window: &Window) -> Result<(), NativeMenuError> {
        self.native.init_for_nsapp();
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn install_native(&self, window: &Window) -> Result<(), NativeMenuError> {
        let hwnd = window_hwnd(window)?;
        // SAFETY: the HWND comes from the live, strongly owned Astrelis Window
        // retained in `self.windows` immediately after successful attachment.
        unsafe { self.native.init_for_hwnd(hwnd) }.map_err(NativeMenuError::from_display)
    }

    #[cfg(target_os = "macos")]
    fn remove_native(&self, _window: &Window) -> Result<(), NativeMenuError> {
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn remove_native(&self, window: &Window) -> Result<(), NativeMenuError> {
        let hwnd = window_hwnd(window)?;
        // SAFETY: the HWND is obtained from the same live window used during
        // attachment and removal occurs before releasing its strong owner.
        unsafe { self.native.remove_for_hwnd(hwnd) }.map_err(NativeMenuError::from_display)
    }
}

impl Drop for ApplicationMenu {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        self.native.remove_for_nsapp();
        #[cfg(target_os = "windows")]
        for window in &self.windows {
            if let Ok(hwnd) = window_hwnd(window) {
                // SAFETY: every stored window is strongly owned and was
                // attached by this menu instance.
                let _ = unsafe { self.native.remove_for_hwnd(hwnd) };
            }
        }
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        INSTALLED.store(false, Ordering::Release);
    }
}

fn build_menu<Message>(
    model: &Menu,
    commands: &CommandRegistry<Message>,
    command_ids: &mut BTreeMap<String, CommandId>,
    items: &mut BTreeMap<CommandId, Vec<NativeCommandItem>>,
    next_id: &mut u64,
) -> Result<Submenu, NativeMenuError> {
    let native = Submenu::new(&model.label, true);
    for entry in &model.entries {
        match entry {
            MenuEntry::Command(command_id) => {
                let command = commands.get(command_id).ok_or_else(|| {
                    NativeMenuError::from_display(format!("unknown command `{command_id}`"))
                })?;
                let native_id = format!("astreon-command:{}:{}", command_id.as_str(), *next_id);
                *next_id += 1;
                command_ids.insert(native_id.clone(), command_id.clone());
                let item = if let Some(checked) = command.checked {
                    let item = CheckMenuItem::with_id(
                        native_id,
                        &command.label,
                        command.enabled,
                        checked,
                        None,
                    );
                    item.set_key_accelerator(key_accelerator(command.shortcut.as_ref()))
                        .map_err(NativeMenuError::from_display)?;
                    native
                        .append(&item)
                        .map_err(NativeMenuError::from_display)?;
                    NativeCommandItem::Check(item)
                } else {
                    let item = MenuItem::with_id(native_id, &command.label, command.enabled, None);
                    item.set_key_accelerator(key_accelerator(command.shortcut.as_ref()))
                        .map_err(NativeMenuError::from_display)?;
                    native
                        .append(&item)
                        .map_err(NativeMenuError::from_display)?;
                    NativeCommandItem::Normal(item)
                };
                items.entry(command_id.clone()).or_default().push(item);
            }
            MenuEntry::Role(role) => {
                let item = predefined(*role);
                native
                    .append(&item)
                    .map_err(NativeMenuError::from_display)?;
            }
            MenuEntry::Submenu(menu) => {
                let submenu = build_menu(menu, commands, command_ids, items, next_id)?;
                native
                    .append(&submenu)
                    .map_err(NativeMenuError::from_display)?;
            }
            MenuEntry::Separator => native
                .append(&PredefinedMenuItem::separator())
                .map_err(NativeMenuError::from_display)?,
        }
    }
    Ok(native)
}

fn predefined(role: MenuRole) -> PredefinedMenuItem {
    match role {
        MenuRole::About => PredefinedMenuItem::about(None, None),
        MenuRole::Copy => PredefinedMenuItem::copy(None),
        MenuRole::Cut => PredefinedMenuItem::cut(None),
        MenuRole::Paste => PredefinedMenuItem::paste(None),
        MenuRole::SelectAll => PredefinedMenuItem::select_all(None),
        MenuRole::Undo => PredefinedMenuItem::undo(None),
        MenuRole::Redo => PredefinedMenuItem::redo(None),
        MenuRole::Minimize => PredefinedMenuItem::minimize(None),
        MenuRole::Maximize => PredefinedMenuItem::maximize(None),
        MenuRole::CloseWindow => PredefinedMenuItem::close_window(None),
        MenuRole::Quit => PredefinedMenuItem::quit(None),
        MenuRole::Fullscreen => PredefinedMenuItem::fullscreen(None),
        MenuRole::Hide => PredefinedMenuItem::hide(None),
        MenuRole::HideOthers => PredefinedMenuItem::hide_others(None),
        MenuRole::ShowAll => PredefinedMenuItem::show_all(None),
        MenuRole::Services => PredefinedMenuItem::services(None),
        MenuRole::BringAllToFront => PredefinedMenuItem::bring_all_to_front(None),
    }
}

fn validate_roles(model: &MenuBar) -> Result<(), NativeMenuError> {
    fn visit(menu: &Menu) -> Result<(), NativeMenuError> {
        for entry in &menu.entries {
            match entry {
                #[cfg(target_os = "windows")]
                MenuEntry::Role(
                    role @ (MenuRole::Fullscreen
                    | MenuRole::Hide
                    | MenuRole::HideOthers
                    | MenuRole::ShowAll
                    | MenuRole::Services
                    | MenuRole::BringAllToFront),
                ) => {
                    return Err(NativeMenuError::from_display(format!(
                        "native menu role `{role:?}` is not supported on Windows"
                    )));
                }
                MenuEntry::Submenu(menu) => visit(menu)?,
                _ => {}
            }
        }
        Ok(())
    }
    for menu in &model.menus {
        visit(menu)?;
    }
    Ok(())
}

fn key_accelerator(shortcut: Option<&Shortcut>) -> Option<KeyAccelerator> {
    let shortcut = shortcut?;
    let key = match &shortcut.key {
        astrelis_platform::Key::Character(value) => Key::Character(value.clone()),
        _ => return None,
    };
    let mut modifiers = Modifiers::empty();
    if shortcut.modifiers.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.modifiers.control {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.modifiers.alt {
        modifiers |= Modifiers::ALT;
    }
    if shortcut.modifiers.super_key {
        modifiers |= Modifiers::SUPER;
    }
    Some(KeyAccelerator::new(
        (!modifiers.is_empty()).then_some(modifiers),
        key,
    ))
}

#[cfg(target_os = "windows")]
fn window_hwnd(window: &Window) -> Result<isize, NativeMenuError> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    match window
        .window_handle()
        .map_err(NativeMenuError::from_display)?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err(NativeMenuError::from_display(
            "Astrelis window does not expose a Win32 handle",
        )),
    }
}
