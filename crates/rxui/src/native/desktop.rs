//! Asynchronous default-application and file-manager requests.
use super::*;
use crate::{Context, Task, TaskError};
use std::path::PathBuf;

/// Failure after scheduling a desktop launch. Success means the OS accepted the
/// launch request, not that a browser loaded a URL or another application read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopError {
    /// The supplied URL has no URI scheme or contains control characters.
    InvalidUrl,
    /// An empty file path cannot be launched/revealed.
    EmptyPath,
    /// The platform launcher returned an error.
    Platform(String),
    /// A scheduled worker panicked.
    Task(TaskError),
}
impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => {
                f.write_str("desktop URL requires a URI scheme and no control characters")
            }
            Self::EmptyPath => f.write_str("desktop file path is empty"),
            Self::Platform(s) => f.write_str(s),
            Self::Task(e) => e.fmt(f),
        }
    }
}
impl Error for DesktopError {}
/// Completion of a desktop launch request.
pub type DesktopResult = Result<(), DesktopError>;
enum Operation {
    Url(String),
    File(PathBuf),
    Reveal(PathBuf),
}
impl Operation {
    fn run(self) -> DesktopResult {
        match self {
            Self::Url(url) => {
                if !valid_url(&url) {
                    return Err(DesktopError::InvalidUrl);
                }
                opener::open(url).map_err(|e| DesktopError::Platform(e.to_string()))
            }
            Self::File(path) => {
                if path.as_os_str().is_empty() {
                    return Err(DesktopError::EmptyPath);
                }
                opener::open(path).map_err(|e| DesktopError::Platform(e.to_string()))
            }
            Self::Reveal(path) => {
                if path.as_os_str().is_empty() {
                    return Err(DesktopError::EmptyPath);
                }
                opener::reveal(path).map_err(|e| DesktopError::Platform(e.to_string()))
            }
        }
    }
}
fn valid_url(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    !url.chars().any(char::is_control)
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}
fn outcome(result: crate::TaskResult<DesktopResult>) -> DesktopResult {
    result.unwrap_or_else(|e| Err(DesktopError::Task(e)))
}
macro_rules! methods {
    ($method:ident, $kind:ident, $input:ty, $doc:literal) => {
        impl AppContext<'_> {
            #[doc = $doc]
            /// Runs on the blocking worker pool and completes in a fresh UI update.
            /// Retain the Task or detach it. Cancellation suppresses delivery but
            /// cannot undo an OS launch that has already begun.
            pub fn $method(
                &mut self,
                value: impl Into<$input>,
                completion: impl FnOnce(DesktopResult, &mut AppContext<'_>) + 'static,
            ) -> Result<Task, ApplicationError> {
                self.native_commands()?;
                let operation = Operation::$kind(value.into());
                self.try_spawn_blocking(
                    move || operation.run(),
                    move |result, cx| completion(outcome(result), cx),
                )
                .map_err(Into::into)
            }
        }
        impl<T: 'static> Context<'_, T> {
            #[doc = $doc]
            /// Weakly delivers to current owner state, using normal owner-scoped
            /// task semantics. Capture a WindowHandle explicitly for window work.
            pub fn $method(
                &mut self,
                value: impl Into<$input>,
                completion: impl FnOnce(&mut T, DesktopResult, &mut Context<'_, T>) + 'static,
            ) -> Result<Task, ApplicationError> {
                self.native_commands()?;
                let operation = Operation::$kind(value.into());
                self.try_spawn_blocking(
                    move || operation.run(),
                    move |state, result, cx| completion(state, outcome(result), cx),
                )
                .map_err(Into::into)
            }
        }
    };
}
methods!(
    open_url,
    Url,
    String,
    "Opens a URI with the OS-configured handler. Custom URI schemes are supported; no shell text is evaluated."
);
methods!(
    open_file,
    File,
    PathBuf,
    "Opens a file with its OS-configured application. The path is passed as data, not shell text."
);
methods!(
    reveal_file,
    Reveal,
    PathBuf,
    "Requests the OS file manager reveal/select a file. Behavior follows the installed file manager; the operation runs off the UI thread."
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackgroundFuture, BlockingJob, SpawnError};
    use std::sync::Mutex;
    #[derive(Default)]
    struct Executor(Mutex<Vec<BlockingJob>>);
    impl TaskExecutor for Executor {
        fn spawn(&self, _: BackgroundFuture) -> Result<(), SpawnError> {
            Err(SpawnError::Rejected(
                "test only supports blocking work".into(),
            ))
        }
        fn spawn_blocking(&self, job: BlockingJob) -> Result<(), SpawnError> {
            self.0.lock().unwrap().push(job);
            Ok(())
        }
    }
    #[test]
    fn services_are_off_thread_fallible_owner_scoped_and_require_a_native_host() {
        let mut runtime = Runtime::new();
        assert!(matches!(
            runtime.update(|cx| cx.open_url("https://example.com", |_, _| {})),
            Err(ApplicationError::NoHost)
        ));
        let commands = Rc::new(Commands::new(runtime.inner.id));
        *runtime.inner.native.borrow_mut() = Some(commands);
        let executor = Arc::new(Executor::default());
        runtime.configure_tasks(executor.clone(), || {}).unwrap();
        let state = runtime.update(|cx| cx.new(|_| None));
        let task = runtime
            .update(|cx| {
                state.update(cx, |_, cx| {
                    cx.open_url("not a URL", |state, result, _| *state = Some(result))
                })
            })
            .unwrap();
        assert!(runtime.update(|cx| state.read(cx).is_none()));
        executor.0.lock().unwrap().pop().unwrap()();
        runtime.poll_tasks();
        assert!(task.is_finished());
        assert_eq!(
            runtime.update(|cx| state.read(cx).clone()),
            Some(Err(DesktopError::InvalidUrl))
        );
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let captured = called.clone();
        let task = runtime
            .update(|cx| {
                state.update(cx, |_, cx| {
                    cx.reveal_file(PathBuf::new(), move |_, _, _| {
                        captured.store(true, std::sync::atomic::Ordering::Release);
                    })
                })
            })
            .unwrap();
        drop(state);
        runtime.synchronize();
        executor.0.lock().unwrap().pop().unwrap()();
        runtime.poll_tasks();
        assert!(task.is_finished());
        assert!(!called.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            Operation::File(PathBuf::new()).run(),
            Err(DesktopError::EmptyPath)
        );
        assert_eq!(
            Operation::Reveal(PathBuf::new()).run(),
            Err(DesktopError::EmptyPath)
        );
    }
    #[test]
    fn launch_arguments_and_worker_failures_have_explicit_results() {
        for url in [
            "https://example.com",
            "mailto:test@example.com",
            "my-app:document/1",
        ] {
            assert!(valid_url(url));
        }
        for url in [
            "",
            "../file.txt",
            ":missing",
            "1scheme:bad",
            "https://example.com\n",
        ] {
            assert!(!valid_url(url));
        }
        assert_eq!(
            outcome(Err(TaskError::Panicked("worker".into()))),
            Err(DesktopError::Task(TaskError::Panicked("worker".into())))
        );
    }
}
