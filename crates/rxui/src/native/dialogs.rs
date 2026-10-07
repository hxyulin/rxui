//! Native dialog descriptions and weak, parent-bound result delivery.
use super::*;
use crate::{Context, MountId};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicU8, Ordering},
};

/// A native operation could not produce a dialog response. File cancellation is
/// represented by Ok(None); RFD also uses None for backend failures it does not expose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogError {
    /// Starting or running the backend failed, including a caught backend panic.
    Backend(String),
}
impl fmt::Display for DialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(s) => f.write_str(s),
        }
    }
}
impl Error for DialogError {}
/// Native dialog completion, separate from request validation errors.
pub type DialogResult<T> = Result<T, DialogError>;
const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const FINISHED: u8 = 2;
struct Control(AtomicU8);
impl Control {
    fn cancel(&self) -> bool {
        self.0
            .compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn claim(&self) -> bool {
        self.0
            .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn finished(&self) -> bool {
        self.0.load(Ordering::Acquire) != RUNNING
    }
}
/// Handle to parent-bound native dialog delivery. Retain it or detach explicitly.
/// Dropping/cancelling suppresses completion and skips a dialog that has not begun.
/// An already open OS dialog is not dismissed; native close/exit waits for its
/// response, keeping borrowed parent handles valid. This handle retains no entity.
#[must_use = "retain the dialog task, or detach it explicitly"]
pub struct DialogTask {
    control: Arc<Control>,
    cancel_on_drop: bool,
}
impl DialogTask {
    /// Cancels delivery once, including a native response queued for the UI thread.
    pub fn cancel(&self) -> bool {
        self.control.cancel()
    }
    /// Whether delivery has completed or been cancelled. An OS dialog may remain
    /// open after cancellation until the user responds to it.
    pub fn is_finished(&self) -> bool {
        self.control.finished()
    }
    /// Lets the request continue without a retained handle. Parent and originating
    /// owner/mount liveness still govern delivery.
    pub fn detach(mut self) {
        self.cancel_on_drop = false;
    }
}
impl Drop for DialogTask {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.control.cancel();
        }
    }
}
impl fmt::Debug for DialogTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DialogTask")
            .field("finished", &self.is_finished())
            .finish()
    }
}
/// Native file picker/save-path description. It never reads or writes file data.
/// A view handler defaults to its source window; application handlers and task
/// completions without a source must select a managed parent explicitly.
///
/// ```no_run
/// use rxui::{AppContext, ApplicationError, FileDialog, WindowHandle};
/// fn choose_file(cx: &mut AppContext<'_>, window: &WindowHandle)
///     -> Result<(), ApplicationError>
/// {
///     cx.pick_file(FileDialog::new().parent(window).filter("Text", ["txt"]),
///         |selection, _cx| {
///             if let Ok(Some(path)) = selection {
///                 // Schedule file I/O with spawn_blocking; the picker returns a path.
///                 println!("Selected {}", path.display());
///             }
///         })?.detach();
///     Ok(())
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct FileDialog {
    parent: Option<WindowHandle>,
    title: String,
    directory: Option<PathBuf>,
    file_name: Option<String>,
    filters: Vec<(String, Vec<String>)>,
}
impl FileDialog {
    /// Creates a picker with native platform defaults.
    pub fn new() -> Self {
        Self::default()
    }
    /// Selects the managed parent, including a window queued for creation.
    pub fn parent(mut self, window: &WindowHandle) -> Self {
        self.parent = Some(window.clone());
        self
    }
    /// Sets the dialog title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    /// Selects the starting directory.
    pub fn directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.directory = Some(path.into());
        self
    }
    /// Selects the proposed basename for Save As. Native extension behavior varies
    /// by platform; inspect the resulting path rather than appending blindly.
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }
    /// Adds an extension filter, without wildcard prefixes or leading dots.
    /// Filter names may be merged by native platforms that do not display them.
    pub fn filter(
        mut self,
        name: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.filters.push((
            name.into(),
            extensions.into_iter().map(Into::into).collect(),
        ));
        self
    }
    fn validate(&self) -> Result<(), ApplicationError> {
        if self.title.contains('\0')
            || self.file_name.as_ref().is_some_and(|s| s.contains('\0'))
            || self.filters.iter().any(|(name, ext)| {
                name.contains('\0')
                    || ext.is_empty()
                    || ext.iter().any(|s| {
                        s.is_empty() || s.starts_with('.') || s.contains(['\0', '*', '/', '\\'])
                    })
            })
        {
            return Err(ApplicationError::InvalidDialogOptions);
        }
        Ok(())
    }
}
/// Native message severity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageLevel {
    /// Informational notification.
    #[default]
    Info,
    /// A decision or potentially destructive change.
    Warning,
    /// A failed operation.
    Error,
}
/// Standard native button sets. Captions/localization follow the platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageButtons {
    /// Acknowledgement.
    #[default]
    Ok,
    /// Acceptance or cancellation.
    OkCancel,
    /// A yes/no decision.
    YesNo,
    /// A yes/no decision with cancellation, suitable for save/discard/cancel.
    YesNoCancel,
}
/// User response to a native message. Closing a dismissible dialog yields Cancel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageResponse {
    /// Acknowledgement/acceptance.
    Ok,
    /// Affirmative choice.
    Yes,
    /// Negative choice.
    No,
    /// Cancelled/dismissed.
    Cancel,
}
/// Native message/confirmation description. It shares file dialogs' managed
/// parent requirement and scoped, asynchronous completion semantics.
#[derive(Clone, Debug, Default)]
pub struct MessageDialog {
    parent: Option<WindowHandle>,
    title: String,
    description: String,
    level: MessageLevel,
    buttons: MessageButtons,
}
impl MessageDialog {
    /// Creates an informational message with an OK button.
    pub fn new() -> Self {
        Self::default()
    }
    /// Selects the managed parent, including a window queued for creation.
    pub fn parent(mut self, window: &WindowHandle) -> Self {
        self.parent = Some(window.clone());
        self
    }
    /// Sets the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    /// Sets the message body.
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = text.into();
        self
    }
    /// Selects native severity styling.
    pub fn level(mut self, level: MessageLevel) -> Self {
        self.level = level;
        self
    }
    /// Selects standard native buttons.
    pub fn buttons(mut self, buttons: MessageButtons) -> Self {
        self.buttons = buttons;
        self
    }
}
#[derive(Clone, Copy)]
pub(super) enum FileKind {
    File,
    Files,
    Folder,
    Folders,
    Save,
}
pub(super) enum Request {
    File(FileDialog, FileKind),
    Message(MessageDialog),
}
pub(super) enum Value {
    File(Option<PathBuf>),
    Files(Option<Vec<PathBuf>>),
    Message(MessageResponse),
}
impl Request {
    fn validate(&self) -> Result<(), ApplicationError> {
        match self {
            Self::File(settings, _) => settings.validate(),
            Self::Message(settings)
                if settings.title.contains('\0') || settings.description.contains('\0') =>
            {
                Err(ApplicationError::InvalidDialogOptions)
            }
            Self::Message(_) => Ok(()),
        }
    }
    fn parent(&self) -> Option<WindowHandle> {
        match self {
            Self::File(s, _) => s.parent.clone(),
            Self::Message(s) => s.parent.clone(),
        }
    }
    fn begin(self, window: &Window) -> Pin<Box<dyn Future<Output = Value> + Send>> {
        match self {
            Self::File(settings, kind) => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_parent(window)
                    .set_title(settings.title);
                if let Some(path) = settings.directory {
                    dialog = dialog.set_directory(path);
                }
                if let Some(name) = settings.file_name {
                    dialog = dialog.set_file_name(name);
                }
                for (name, extensions) in settings.filters {
                    dialog = dialog.add_filter(name, &extensions);
                }
                match kind {
                    FileKind::File => {
                        let future = dialog.pick_file();
                        Box::pin(
                            async move { Value::File(future.await.map(|f| f.path().to_owned())) },
                        )
                    }
                    FileKind::Files => {
                        let future = dialog.pick_files();
                        Box::pin(async move {
                            Value::Files(future.await.map(|files| {
                                files.into_iter().map(|f| f.path().to_owned()).collect()
                            }))
                        })
                    }
                    FileKind::Folder => {
                        let future = dialog.pick_folder();
                        Box::pin(
                            async move { Value::File(future.await.map(|f| f.path().to_owned())) },
                        )
                    }
                    FileKind::Folders => {
                        let future = dialog.pick_folders();
                        Box::pin(async move {
                            Value::Files(future.await.map(|files| {
                                files.into_iter().map(|f| f.path().to_owned()).collect()
                            }))
                        })
                    }
                    FileKind::Save => {
                        let future = dialog.save_file();
                        Box::pin(
                            async move { Value::File(future.await.map(|f| f.path().to_owned())) },
                        )
                    }
                }
            }
            Self::Message(settings) => {
                let level = match settings.level {
                    MessageLevel::Info => rfd::MessageLevel::Info,
                    MessageLevel::Warning => rfd::MessageLevel::Warning,
                    MessageLevel::Error => rfd::MessageLevel::Error,
                };
                let buttons = match settings.buttons {
                    MessageButtons::Ok => rfd::MessageButtons::Ok,
                    MessageButtons::OkCancel => rfd::MessageButtons::OkCancel,
                    MessageButtons::YesNo => rfd::MessageButtons::YesNo,
                    MessageButtons::YesNoCancel => rfd::MessageButtons::YesNoCancel,
                };
                let future = rfd::AsyncMessageDialog::new()
                    .set_parent(window)
                    .set_title(settings.title)
                    .set_description(settings.description)
                    .set_level(level)
                    .set_buttons(buttons)
                    .show();
                Box::pin(async move {
                    Value::Message(match future.await {
                        rfd::MessageDialogResult::Ok => MessageResponse::Ok,
                        rfd::MessageDialogResult::Yes => MessageResponse::Yes,
                        rfd::MessageDialogResult::No => MessageResponse::No,
                        _ => MessageResponse::Cancel,
                    })
                })
            }
        }
    }
}
type Callback = dyn FnOnce(DialogResult<Value>, &mut AppContext<'_>);
struct Record {
    parent: WindowHandle,
    scope: Option<MountId>,
    live: Rc<dyn Fn() -> bool>,
    request: Option<Request>,
    control: Arc<Control>,
    callback: Box<Callback>,
}
/// State remains on the UI thread, including all callbacks and weak entity handles.
#[derive(Default)]
pub(super) struct State {
    next: u64,
    records: HashMap<u64, Record>,
}
impl State {
    pub(super) fn busy(&self, parent: WindowId) -> bool {
        self.records.values().any(|r| r.parent.id() == parent)
    }
    pub(super) fn running(&self) -> bool {
        self.records.values().any(|r| r.request.is_none())
    }
    pub(super) fn prune(&mut self, runtime: &crate::runtime::RuntimeInner, exiting: bool) {
        self.records.retain(|_, r| {
            if r.request.is_none() {
                return true;
            }
            let live = !exiting
                && !r.control.finished()
                && !r.parent.is_closed()
                && (r.live)()
                && r.scope.is_none_or(|s| runtime.mount_live(s));
            if !live {
                r.control.cancel();
            }
            live
        });
    }
    pub(super) fn abandon(&mut self) {
        for r in self.records.values() {
            r.control.cancel();
        }
    }
}
fn enqueue(
    cx: &mut AppContext<'_>,
    request: Request,
    live: Rc<dyn Fn() -> bool>,
    callback: impl FnOnce(DialogResult<Value>, &mut AppContext<'_>) + 'static,
) -> Result<DialogTask, ApplicationError> {
    let commands = cx.native_commands()?;
    request.validate()?;
    let parent = request
        .parent()
        .or_else(|| cx.window())
        .ok_or(ApplicationError::NoSourceWindow)?;
    if parent.id().runtime != commands.runtime {
        return Err(crate::AccessError::WrongRuntime.into());
    }
    if parent.is_closed() {
        return Err(ApplicationError::ClosedWindow);
    }
    let mut dialogs = commands.dialogs.borrow_mut();
    dialogs.prune(cx.runtime, false);
    if dialogs.busy(parent.id()) {
        return Err(ApplicationError::DialogBusy);
    }
    dialogs.next = dialogs
        .next
        .checked_add(1)
        .expect("RXUI dialog identity exhausted");
    let id = dialogs.next;
    let control = Arc::new(Control(AtomicU8::new(RUNNING)));
    dialogs.records.insert(
        id,
        Record {
            parent,
            scope: cx.dispatch_mount,
            live,
            request: Some(request),
            control: control.clone(),
            callback: Box::new(callback),
        },
    );
    commands.queue.borrow_mut().push_back(Command::Dialog(id));
    Ok(DialogTask {
        control,
        cancel_on_drop: true,
    })
}
macro_rules! file_methods {
    ($method:ident, $kind:ident, $value:ident, $output:ty, $doc:literal) => {
        impl AppContext<'_> {
            #[doc = $doc]
            pub fn $method(
                &mut self,
                options: FileDialog,
                callback: impl FnOnce(DialogResult<$output>, &mut AppContext<'_>) + 'static,
            ) -> Result<DialogTask, ApplicationError> {
                enqueue(
                    self,
                    Request::File(options, FileKind::$kind),
                    Rc::new(|| true),
                    move |value, cx| {
                        callback(
                            value.map(|v| match v {
                                Value::$value(v) => v,
                                _ => unreachable!("typed dialog response"),
                            }),
                            cx,
                        )
                    },
                )
            }
        }
        impl<T: 'static> Context<'_, T> {
            #[doc = $doc]
            /// Completion weakly targets current state and restores the request's
            /// source window. A disposed origin/closed parent suppresses delivery.
            pub fn $method(
                &mut self,
                options: FileDialog,
                callback: impl FnOnce(&mut T, DialogResult<$output>, &mut Context<'_, T>) + 'static,
            ) -> Result<DialogTask, ApplicationError> {
                let owner = self.entity();
                let live = owner.clone();
                enqueue(
                    self,
                    Request::File(options, FileKind::$kind),
                    Rc::new(move || live.upgrade().is_some()),
                    move |value, cx| {
                        if let Some(entity) = owner.upgrade() {
                            entity.update(cx, |s, cx| {
                                callback(
                                    s,
                                    value.map(|v| match v {
                                        Value::$value(v) => v,
                                        _ => unreachable!("typed dialog response"),
                                    }),
                                    cx,
                                )
                            });
                        }
                    },
                )
            }
        }
    };
}
file_methods!(
    pick_file,
    File,
    File,
    Option<PathBuf>,
    "Queues a native single-file picker. Cancellation/backend absence yields None; no file data is read."
);
file_methods!(
    pick_files,
    Files,
    Files,
    Option<Vec<PathBuf>>,
    "Queues a native multiple-file picker without reading file data."
);
file_methods!(
    pick_folder,
    Folder,
    File,
    Option<PathBuf>,
    "Queues a native directory picker."
);
file_methods!(
    pick_folders,
    Folders,
    Files,
    Option<Vec<PathBuf>>,
    "Queues a native multiple-directory picker."
);
file_methods!(
    save_file,
    Save,
    File,
    Option<PathBuf>,
    "Queues a native Save As path picker, including native overwrite confirmation. No file is written."
);
impl AppContext<'_> {
    /// Queues a native message/confirmation with standard platform buttons.
    pub fn show_message(
        &mut self,
        options: MessageDialog,
        callback: impl FnOnce(DialogResult<MessageResponse>, &mut AppContext<'_>) + 'static,
    ) -> Result<DialogTask, ApplicationError> {
        enqueue(
            self,
            Request::Message(options),
            Rc::new(|| true),
            move |value, cx| {
                callback(
                    value.map(|v| match v {
                        Value::Message(v) => v,
                        _ => unreachable!("typed dialog response"),
                    }),
                    cx,
                )
            },
        )
    }
}
impl<T: 'static> Context<'_, T> {
    /// Queues a native message/confirmation weakly targeting current state, with
    /// the same parent/source/liveness rules as file pickers.
    pub fn show_message(
        &mut self,
        options: MessageDialog,
        callback: impl FnOnce(&mut T, DialogResult<MessageResponse>, &mut Context<'_, T>) + 'static,
    ) -> Result<DialogTask, ApplicationError> {
        let owner = self.entity();
        let live = owner.clone();
        enqueue(
            self,
            Request::Message(options),
            Rc::new(move || live.upgrade().is_some()),
            move |value, cx| {
                if let Some(entity) = owner.upgrade() {
                    entity.update(cx, |s, cx| {
                        callback(
                            s,
                            value.map(|v| match v {
                                Value::Message(v) => v,
                                _ => unreachable!("typed dialog response"),
                            }),
                            cx,
                        )
                    });
                }
            },
        )
    }
}
pub(super) struct Packet {
    id: u64,
    result: DialogResult<Value>,
}
fn panic_message(panic: Box<dyn std::any::Any + Send>) -> DialogError {
    DialogError::Backend(
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).into()))
            .unwrap_or_else(|| "native dialog backend panicked".into()),
    )
}
impl<F> Host<F> {
    /// Starts native UI outside all entity leases. A dedicated sleeping waiter is
    /// independent of cancellable task pools, retaining the parent even if host
    /// teardown fails. Native dialogs already own platform UI/COM resources.
    pub(super) fn start_dialog(
        &mut self,
        cx: &mut NativeContext<'_, Wake>,
        id: u64,
    ) -> Result<bool, ApplicationError> {
        let mut dialogs = self.commands.dialogs.borrow_mut();
        let Some(record) = dialogs.records.get_mut(&id) else {
            return Ok(true);
        };
        let Some(window) = record.parent.native_window() else {
            return Ok(false);
        };
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<Pin<Box<dyn Future<Output = Value> + Send>>>(1);
        let proxy = cx.proxy();
        let keep_parent = window.clone();
        // Reserve execution before displaying native UI. The waiter is deliberately
        // not aborted on delivery cancellation; it must keep borrowed HWNDs valid.
        let worker = std::thread::Builder::new()
            .name("rxui-dialog".into())
            .spawn(move || {
                if let Ok(future) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        futures_lite::future::block_on(future)
                    }))
                    .map_err(panic_message);
                    let _ = proxy.send_event(Wake::Dialog(Packet { id, result }));
                }
                drop(keep_parent);
            });
        let result = match worker {
            Err(error) => Err(DialogError::Backend(error.to_string())),
            Ok(_) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                record
                    .request
                    .take()
                    .expect("unstarted dialog")
                    .begin(&window)
            }))
            .map_err(panic_message)
            .map(|future| sender.send(future).expect("reserved native dialog waiter")),
        };
        drop(dialogs);
        if let Err(error) = result {
            self.finish_dialog(Packet {
                id,
                result: Err(error),
            });
        }
        Ok(true)
    }
    pub(super) fn finish_dialog(&mut self, packet: Packet) {
        deliver(&self.commands, &mut self.runtime, packet);
    }
}
fn deliver(commands: &Rc<Commands>, runtime: &mut Runtime, packet: Packet) {
    let record = commands.dialogs.borrow_mut().records.remove(&packet.id);
    if let Some(record) = record
        && record.control.claim()
        && !commands.exited.get()
        && !record.parent.is_closed()
        && (record.live)()
        && record.scope.is_none_or(|s| runtime.inner.mount_live(s))
    {
        let source = record.scope.or_else(|| {
            commands
                .mounts
                .borrow()
                .iter()
                .find(|(_, window)| **window == record.parent.id())
                .map(|(mount, _)| *mount)
        });
        runtime.update(|cx| {
            cx.dispatch_mount = source;
            (record.callback)(packet.result, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntoElement, ViewContext};
    struct Page {
        path: Option<PathBuf>,
        source: Option<WindowId>,
        calls: usize,
    }
    impl View for Page {
        fn view(&self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            crate::label("Dialog owner")
        }
    }
    fn setup() -> (Runtime, Rc<Commands>, Entity<Page>, WindowHandle) {
        let mut runtime = Runtime::new();
        let commands = Rc::new(Commands::new(runtime.inner.id));
        *runtime.inner.native.borrow_mut() = Some(commands.clone());
        let (page, window) = runtime.update(|cx| {
            let page = cx.new(|_| Page {
                path: None,
                source: None,
                calls: 0,
            });
            let window = cx.open_window(WindowOptions::new(), page.clone()).unwrap();
            (page, window)
        });
        (runtime, commands, page, window)
    }
    fn queued(commands: &Commands) -> u64 {
        *commands.dialogs.borrow().records.keys().next().unwrap()
    }
    #[test]
    fn validation_precedes_reservation_and_all_picker_modes_have_typed_results() {
        let mut no_host = Runtime::new();
        assert!(matches!(
            no_host.update(|cx| cx.pick_file(FileDialog::new(), |_, _| {})),
            Err(ApplicationError::NoHost)
        ));
        let (mut r, commands, _, parent) = setup();
        assert!(matches!(
            r.update(|cx| cx.pick_file(FileDialog::new(), |_, _| {})),
            Err(ApplicationError::NoSourceWindow)
        ));
        for options in [
            FileDialog::new().parent(&parent).filter("Text", ["*.txt"]),
            FileDialog::new()
                .parent(&parent)
                .filter("Text", [] as [&str; 0]),
            FileDialog::new().parent(&parent).title("bad\0title"),
        ] {
            assert!(matches!(
                r.update(|cx| cx.pick_file(options, |_, _| {})),
                Err(ApplicationError::InvalidDialogOptions)
            ));
            assert!(!commands.dialogs.borrow().busy(parent.id()));
        }
        let mut foreign = Runtime::new();
        *foreign.inner.native.borrow_mut() = Some(Rc::new(Commands::new(foreign.inner.id)));
        assert!(matches!(
            foreign.update(|cx| cx.pick_file(FileDialog::new().parent(&parent), |_, _| {})),
            Err(ApplicationError::Ui(UiError::Access(
                crate::AccessError::WrongRuntime
            )))
        ));
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let task = r
            .update(|cx| {
                cx.pick_file(FileDialog::new().parent(&parent), move |result, _| {
                    assert_eq!(result.unwrap(), Some(PathBuf::from("selected.txt")));
                    seen.set(seen.get() + 1);
                })
            })
            .unwrap();
        assert!(matches!(
            r.update(|cx| cx.show_message(MessageDialog::new().parent(&parent), |_, _| {})),
            Err(ApplicationError::DialogBusy)
        ));
        deliver(
            &commands,
            &mut r,
            Packet {
                id: queued(&commands),
                result: Ok(Value::File(Some("selected.txt".into()))),
            },
        );
        assert!(task.is_finished());
        assert_eq!(count.get(), 1);
        assert!(!commands.dialogs.borrow().busy(parent.id()));
        macro_rules! complete {
            ($method:ident, $value:expr) => {{
                let task = r
                    .update(|cx| {
                        cx.$method(FileDialog::new().parent(&parent), |result, _| {
                            assert!(result.unwrap().is_none())
                        })
                    })
                    .unwrap();
                deliver(
                    &commands,
                    &mut r,
                    Packet {
                        id: queued(&commands),
                        result: Ok($value),
                    },
                );
                assert!(task.is_finished());
            }};
        }
        complete!(pick_files, Value::Files(None));
        complete!(pick_folder, Value::File(None));
        complete!(pick_folders, Value::Files(None));
        complete!(save_file, Value::File(None));
        let task = r
            .update(|cx| {
                cx.show_message(MessageDialog::new().parent(&parent), |result, _| {
                    assert_eq!(result.unwrap(), MessageResponse::Cancel)
                })
            })
            .unwrap();
        deliver(
            &commands,
            &mut r,
            Packet {
                id: queued(&commands),
                result: Ok(Value::Message(MessageResponse::Cancel)),
            },
        );
        assert!(task.is_finished());
    }
    #[test]
    fn cancellation_disposal_and_parent_close_suppress_stale_delivery() {
        let (mut r, commands, page, parent) = setup();
        let task = r
            .update(|cx| {
                page.update(cx, |_, cx| {
                    cx.pick_file(FileDialog::new().parent(&parent), |s, _, _| s.calls += 1)
                })
            })
            .unwrap();
        let first = queued(&commands);
        assert!(task.cancel());
        assert!(!task.cancel());
        deliver(
            &commands,
            &mut r,
            Packet {
                id: first,
                result: Ok(Value::File(None)),
            },
        );
        assert_eq!(r.update(|cx| page.read(cx).calls), 0);
        let task = r
            .update(|cx| {
                page.update(cx, |_, cx| {
                    cx.pick_file(FileDialog::new().parent(&parent), |s, _, _| s.calls += 1)
                })
            })
            .unwrap();
        parent.life.closing.set(true);
        deliver(
            &commands,
            &mut r,
            Packet {
                id: queued(&commands),
                result: Ok(Value::File(None)),
            },
        );
        assert!(task.is_finished());
        assert_eq!(r.update(|cx| page.read(cx).calls), 0);
        parent.life.closing.set(false);
        let task = r
            .update(|cx| {
                page.update(cx, |_, cx| {
                    cx.pick_file(FileDialog::new().parent(&parent), |_, _, _| {
                        panic!("disposed owner")
                    })
                })
            })
            .unwrap();
        commands.queue.borrow_mut().clear(); // Release queued window factory ownership.
        drop(page);
        r.synchronize();
        commands.dialogs.borrow_mut().prune(&r.inner, false);
        assert!(task.is_finished());
        assert!(!commands.dialogs.borrow().busy(parent.id()));
    }
    #[test]
    fn scoped_completion_restores_window_and_does_not_retain_a_disposed_mount() {
        let (mut r, commands, page, parent) = setup();
        let mount = r.update(|cx| cx.mount(&page).unwrap());
        commands.mounts.borrow_mut().insert(mount.id(), parent.id());
        let listener = r
            .evaluate(&mount, |_, cx| {
                cx.listener(|_: &mut Page, _: &(), cx| {
                    cx.pick_file(FileDialog::new(), |s, result, cx| {
                        s.calls += 1;
                        s.path = result.unwrap();
                        s.source = cx.window().map(|w| w.id());
                    })
                    .unwrap()
                    .detach();
                })
            })
            .unwrap();
        r.update(|cx| listener.dispatch(&(), cx)).unwrap();
        deliver(
            &commands,
            &mut r,
            Packet {
                id: queued(&commands),
                result: Ok(Value::File(Some("selected.txt".into()))),
            },
        );
        assert_eq!(
            r.update(|cx| (page.read(cx).calls, page.read(cx).source)),
            (1, Some(parent.id()))
        );
        r.update(|cx| listener.dispatch(&(), cx)).unwrap();
        drop(mount);
        deliver(
            &commands,
            &mut r,
            Packet {
                id: queued(&commands),
                result: Ok(Value::File(None)),
            },
        );
        assert_eq!(r.update(|cx| page.read(cx).calls), 1);
    }
    #[test]
    fn cancelling_open_dialog_delivery_keeps_parent_busy_until_native_response() {
        let (mut r, commands, _, parent) = setup();
        let task = r
            .update(|cx| {
                cx.pick_file(FileDialog::new().parent(&parent), |_, _| {
                    panic!("cancelled")
                })
            })
            .unwrap();
        let id = queued(&commands);
        commands
            .dialogs
            .borrow_mut()
            .records
            .get_mut(&id)
            .unwrap()
            .request
            .take(); // Native dialog began.
        task.cancel();
        commands.dialogs.borrow_mut().prune(&r.inner, false);
        assert!(commands.dialogs.borrow().running());
        assert!(commands.dialogs.borrow().busy(parent.id()));
        deliver(
            &commands,
            &mut r,
            Packet {
                id,
                result: Ok(Value::File(None)),
            },
        );
        assert!(!commands.dialogs.borrow().running());
        assert!(!commands.dialogs.borrow().busy(parent.id()));
    }
}
