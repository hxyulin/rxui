//! Host-executed component services and controlled undo history.

use std::{any::Any, collections::VecDeque, num::NonZeroUsize};

/// An erased component action produced by a host service.
pub type ServiceAction = Box<dyn Any>;

/// A clipboard read whose result is mapped back into its owning component.
pub struct ClipboardReadRequest {
    complete: Box<dyn FnOnce(Option<String>) -> ServiceAction>,
}

impl ClipboardReadRequest {
    pub(crate) fn new(complete: impl FnOnce(Option<String>) -> ServiceAction + 'static) -> Self {
        Self {
            complete: Box::new(complete),
        }
    }

    /// Maps clipboard contents into the correctly routed component action.
    pub fn complete(self, contents: Option<String>) -> ServiceAction {
        (self.complete)(contents)
    }
}

/// Sendable background work independent of the UI thread.
pub struct BackgroundWork {
    run: Box<dyn FnOnce() -> Box<dyn Any + Send> + Send>,
}

impl BackgroundWork {
    /// Runs the background operation.
    pub fn run(self) -> Box<dyn Any + Send> {
        (self.run)()
    }
}

/// Main-thread mapping from background output to a component action.
pub struct BackgroundCompletion {
    complete: Box<dyn FnOnce(Box<dyn Any + Send>) -> ServiceAction>,
}

impl BackgroundCompletion {
    /// Maps a completed background result into its owning component action.
    pub fn complete(self, output: Box<dyn Any + Send>) -> ServiceAction {
        (self.complete)(output)
    }
}

/// A background task split into sendable work and main-thread completion.
pub struct BackgroundTaskRequest {
    work: BackgroundWork,
    completion: BackgroundCompletion,
}

impl BackgroundTaskRequest {
    pub(crate) fn new<Result>(
        work: impl FnOnce() -> Result + Send + 'static,
        complete: impl FnOnce(Result) -> ServiceAction + 'static,
    ) -> Self
    where
        Result: Send + 'static,
    {
        Self {
            work: BackgroundWork {
                run: Box::new(move || Box::new(work())),
            },
            completion: BackgroundCompletion {
                complete: Box::new(move |output| {
                    let output = output
                        .downcast::<Result>()
                        .expect("background result type is preserved by the request");
                    complete(*output)
                }),
            },
        }
    }

    /// Separates work that may move to a worker thread from main-thread mapping.
    pub fn split(self) -> (BackgroundWork, BackgroundCompletion) {
        (self.work, self.completion)
    }

    /// Runs this request synchronously, useful for deterministic tests.
    pub fn run(self) -> ServiceAction {
        let (work, completion) = self.split();
        completion.complete(work.run())
    }
}

/// Host operation requested while reducing a component action.
pub enum ComponentServiceRequest {
    /// Replace host clipboard text.
    ClipboardWrite(String),
    /// Read host clipboard text and dispatch the mapped result.
    ClipboardRead(ClipboardReadRequest),
    /// Execute work away from the UI thread and dispatch its mapped result.
    BackgroundTask(BackgroundTaskRequest),
}

/// Clipboard abstraction used by deterministic component hosts.
pub trait Clipboard {
    /// Returns clipboard text when available.
    fn read_text(&mut self) -> Option<String>;

    /// Replaces clipboard text.
    fn write_text(&mut self, text: String);
}

/// In-memory clipboard for tests, tools, and headless applications.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryClipboard {
    text: Option<String>,
}

impl MemoryClipboard {
    /// Creates a clipboard with optional initial text.
    pub fn new(text: impl Into<Option<String>>) -> Self {
        Self { text: text.into() }
    }

    /// Reads the current value without mutating it.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

impl Clipboard for MemoryClipboard {
    fn read_text(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn write_text(&mut self, text: String) {
        self.text = Some(text);
    }
}

/// Bounded snapshot-based undo and redo history.
#[derive(Clone, Debug)]
pub struct UndoHistory<State> {
    undo: VecDeque<State>,
    redo: Vec<State>,
    limit: NonZeroUsize,
}

impl<State> UndoHistory<State> {
    /// Creates history retaining at most `limit` undo snapshots.
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit,
        }
    }

    /// Records state immediately before an application mutation.
    pub fn checkpoint(&mut self, state: State) {
        if self.undo.len() == self.limit.get() {
            self.undo.pop_front();
        }
        self.undo.push_back(state);
        self.redo.clear();
    }

    /// Restores the most recent checkpoint and records the current state for redo.
    pub fn undo(&mut self, current: &mut State) -> bool {
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push(std::mem::replace(current, previous));
        true
    }

    /// Restores the most recently undone state.
    pub fn redo(&mut self, current: &mut State) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        if self.undo.len() == self.limit.get() {
            self.undo.pop_front();
        }
        self.undo.push_back(std::mem::replace(current, next));
        true
    }

    /// Returns whether undo is currently available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Returns whether redo is currently available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Discards all recorded snapshots.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
