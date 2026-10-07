use super::*;
#[cfg(target_os = "macos")]
use crate::NativeMenuRole;
use crate::{
    CommandId, CommandInfo, NativeMenuBar, Shortcut,
    native_menus::{Entry, NativeMenu},
};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::{Mutex, Once};

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;

// Muda's event handler can only be installed once. Keep the wake destination in
// replaceable storage instead of permanently retaining a finished application's loop.
static INSTALL: Once = Once::new();
static PROXY: Mutex<Option<astrelis_winit::winit::event_loop::EventLoopProxy<Wake>>> =
    Mutex::new(None);
fn install(proxy: astrelis_winit::winit::event_loop::EventLoopProxy<Wake>) {
    *PROXY.lock().expect("menu proxy") = Some(proxy);
    INSTALL.call_once(|| {
        MenuEvent::set_event_handler(Some(|event| {
            if let Some(proxy) = &*PROXY.lock().expect("menu proxy") {
                let _ = proxy.send_event(Wake::Menu(event));
            }
        }))
    });
}
fn literal(text: &str) -> String {
    text.replace('&', "&&")
}
struct Item {
    native: MenuItem,
    command: CommandId,
    fallback: String,
    current: Option<(CommandInfo, Option<Shortcut>)>,
}
pub(super) struct Backend {
    menu: Menu,
    items: Vec<Item>,
}
impl Backend {
    fn new(model: &NativeMenuBar) -> Result<Self, ApplicationError> {
        let mut result = Self {
            menu: Menu::new(),
            items: Vec::new(),
        };
        for menu in &model.menus {
            let submenu = result.submenu(menu)?;
            result.menu.append(&submenu).map_err(native_error)?;
        }
        Ok(result)
    }
    fn submenu(&mut self, model: &NativeMenu) -> Result<Submenu, ApplicationError> {
        let menu = Submenu::new(literal(&model.label), true);
        for entry in &model.entries {
            match entry {
                Entry::Command(command, fallback) => {
                    let native = MenuItem::new(literal(fallback), false, None);
                    menu.append(&native).map_err(native_error)?;
                    self.items.push(Item {
                        native,
                        command: *command,
                        fallback: fallback.clone(),
                        current: None,
                    });
                }
                Entry::Separator => menu
                    .append(&PredefinedMenuItem::separator())
                    .map_err(native_error)?,
                Entry::Submenu(model) => {
                    menu.append(&self.submenu(model)?).map_err(native_error)?
                }
                Entry::Role(role) => {
                    #[cfg(target_os = "macos")]
                    {
                        let item = match role {
                            NativeMenuRole::About => PredefinedMenuItem::about(None, None),
                            NativeMenuRole::Services => PredefinedMenuItem::services(None),
                            NativeMenuRole::Hide => PredefinedMenuItem::hide(None),
                            NativeMenuRole::HideOthers => PredefinedMenuItem::hide_others(None),
                            NativeMenuRole::ShowAll => PredefinedMenuItem::show_all(None),
                        };
                        menu.append(&item).map_err(native_error)?;
                    }
                    #[cfg(target_os = "windows")]
                    let _ = role;
                }
            }
        }
        Ok(menu)
    }
    fn update(
        &mut self,
        mut query: impl FnMut(CommandId) -> Option<CommandInfo>,
        mut winner: impl FnMut(&Shortcut) -> Option<CommandId>,
    ) -> Result<(), ApplicationError> {
        let mut installed: Vec<Shortcut> = Vec::new();
        for item in &mut self.items {
            let info = query(item.command).unwrap_or_else(|| CommandInfo {
                label: item.fallback.clone(),
                enabled: false,
                shortcuts: Vec::new(),
            });
            let shortcut = info
                .shortcuts
                .iter()
                .find(|s| {
                    accelerator(s).is_some()
                        && !installed.contains(*s)
                        && winner(s).is_none_or(|id| id == item.command)
                })
                .cloned();
            if let Some(s) = &shortcut {
                installed.push(s.clone());
            }
            let next = (info, shortcut);
            if item.current.as_ref() != Some(&next) {
                if item
                    .current
                    .as_ref()
                    .is_none_or(|c| c.0.label != next.0.label)
                {
                    item.native.set_text(literal(&next.0.label));
                }
                if item
                    .current
                    .as_ref()
                    .is_none_or(|c| c.0.enabled != next.0.enabled)
                {
                    item.native.set_enabled(next.0.enabled);
                }
                if item.current.as_ref().is_none_or(|c| c.1 != next.1) {
                    item.native
                        .set_key_accelerator(next.1.as_ref().and_then(accelerator))
                        .map_err(native_error)?;
                }
                item.current = Some(next);
            }
        }
        Ok(())
    }
    fn command(&self, id: &muda::MenuId) -> Option<CommandId> {
        self.items
            .iter()
            .find(|i| i.native.id() == id)
            .map(|i| i.command)
    }
}
fn native_error(error: muda::Error) -> ApplicationError {
    ApplicationError::Native(Box::new(error))
}
fn accelerator(shortcut: &Shortcut) -> Option<muda::accelerator::KeyAccelerator> {
    use crate::KeyboardKey as K;
    use muda::accelerator::{Key, KeyAccelerator, Modifiers, NamedKey};
    let key = match &shortcut.key {
        K::Character(s) if s.chars().count() == 1 => Key::Character(s.to_lowercase()),
        K::Space => Key::Character(" ".into()),
        K::Escape => Key::Named(NamedKey::Escape),
        K::Enter => Key::Named(NamedKey::Enter),
        K::Tab => Key::Named(NamedKey::Tab),
        K::Backspace => Key::Named(NamedKey::Backspace),
        K::Delete => Key::Named(NamedKey::Delete),
        K::ArrowLeft => Key::Named(NamedKey::ArrowLeft),
        K::ArrowRight => Key::Named(NamedKey::ArrowRight),
        K::ArrowUp => Key::Named(NamedKey::ArrowUp),
        K::ArrowDown => Key::Named(NamedKey::ArrowDown),
        K::Home => Key::Named(NamedKey::Home),
        K::End => Key::Named(NamedKey::End),
        K::PageUp => Key::Named(NamedKey::PageUp),
        K::PageDown => Key::Named(NamedKey::PageDown),
        _ => return None,
    };
    // Unmodified keys stay in RXUI: a menu equivalent must not intercept typing,
    // Tab navigation or arrow movement before routed input listeners see them.
    if !shortcut.modifiers.control && !shortcut.modifiers.meta && !shortcut.modifiers.alt {
        return None;
    }
    let mut modifiers = Modifiers::empty();
    if shortcut.modifiers.control {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.modifiers.meta {
        modifiers |= Modifiers::META;
    }
    if shortcut.modifiers.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.modifiers.alt {
        modifiers |= Modifiers::ALT;
    }
    Some(KeyAccelerator::new(modifiers, key))
}

#[cfg(target_os = "windows")]
type WindowMenus = Rc<RefCell<HashMap<NativeWindowId, (Arc<Window>, Backend)>>>;

pub(super) struct Menus {
    model: NativeMenuBar,
    #[cfg(target_os = "macos")]
    backend: Option<Backend>,
    #[cfg(target_os = "windows")]
    windows: WindowMenus,
}
impl Menus {
    pub(super) fn new(model: NativeMenuBar) -> Self {
        Self {
            model,
            #[cfg(target_os = "macos")]
            backend: None,
            #[cfg(target_os = "windows")]
            windows: Rc::new(RefCell::new(HashMap::new())),
        }
    }
    pub(super) fn start(
        &mut self,
        proxy: astrelis_winit::winit::event_loop::EventLoopProxy<Wake>,
    ) -> Result<(), ApplicationError> {
        install(proxy);
        #[cfg(target_os = "macos")]
        if self.backend.is_none() {
            let backend = Backend::new(&self.model)?;
            backend.menu.init_for_nsapp();
            self.backend = Some(backend);
        }
        Ok(())
    }
    pub(super) fn attach(
        &mut self,
        id: NativeWindowId,
        window: Arc<Window>,
    ) -> Result<(), ApplicationError> {
        #[cfg(target_os = "windows")]
        {
            let backend = Backend::new(&self.model)?;
            windows::attach(&backend.menu, &window)?;
            self.windows.borrow_mut().insert(id, (window, backend));
        }
        #[cfg(target_os = "macos")]
        let _ = (id, window);
        Ok(())
    }
    pub(super) fn detach(&mut self, id: NativeWindowId) {
        #[cfg(target_os = "windows")]
        if let Some((window, backend)) = self.windows.borrow_mut().remove(&id) {
            windows::detach(&backend.menu, &window);
        }
        #[cfg(target_os = "macos")]
        let _ = id;
    }
    pub(super) fn update(
        &mut self,
        id: Option<NativeWindowId>,
        query: impl FnMut(CommandId) -> Option<CommandInfo>,
        winner: impl FnMut(&Shortcut) -> Option<CommandId>,
    ) -> Result<(), ApplicationError> {
        #[cfg(target_os = "macos")]
        {
            let _ = id;
            if let Some(backend) = &mut self.backend {
                backend.update(query, winner)?;
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(id) = id
            && let Some((_, backend)) = self.windows.borrow_mut().get_mut(&id)
        {
            backend.update(query, winner)?;
        }
        Ok(())
    }
    pub(super) fn resolve(
        &self,
        event: &MenuEvent,
        focused: Option<NativeWindowId>,
    ) -> Option<(Option<NativeWindowId>, CommandId)> {
        #[cfg(target_os = "macos")]
        {
            self.backend
                .as_ref()?
                .command(&event.id)
                .map(|c| (focused, c))
        }
        #[cfg(target_os = "windows")]
        {
            let _ = focused;
            self.windows
                .borrow()
                .iter()
                .find_map(|(id, (_, backend))| backend.command(&event.id).map(|c| (Some(*id), c)))
        }
    }
    #[cfg(target_os = "windows")]
    pub(super) fn hook(
        &self,
        builder: &mut astrelis_winit::winit::event_loop::EventLoopBuilder<Wake>,
    ) {
        windows::hook(builder, self.windows.clone());
    }
}
impl Drop for Menus {
    fn drop(&mut self) {
        *PROXY.lock().expect("menu proxy") = None;
        #[cfg(target_os = "macos")]
        if let Some(backend) = &self.backend {
            backend.menu.remove_for_nsapp();
        }
        #[cfg(target_os = "windows")]
        for (_, (window, backend)) in self.windows.borrow_mut().drain() {
            windows::detach(&backend.menu, &window);
        }
    }
}
