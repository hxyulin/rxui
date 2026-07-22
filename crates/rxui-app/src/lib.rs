//! Desktop application hosting and shared application commands.

#![warn(missing_docs)]

mod command;
mod error;
mod host;
mod instrumentation;
mod menu;
mod message;
mod runner;
mod state;
mod subscription;
mod undo;

pub use command::{Command, CommandError, CommandId, CommandRegistry, CommandRouter, Shortcut};
pub use error::{Error, Result};
pub use host::{GraphicsContext, HostError, HostStatus, HostUpdate, WindowHost, WindowHostOptions};
#[doc(hidden)]
pub use instrumentation::{InstrumentationState, PendingMessageTrace};
pub use instrumentation::{
    MessageDispatch, MessageMetadata, MessageOrigin, MessageOutcome, MessageTrace,
    MessageTraceIdentity, QueuedMessage, RuntimeEvent, RuntimeInstrumentationConfig,
    RuntimeLifecycleEvent, RuntimeLifecycleTrace, RuntimeObserver, RuntimeResource,
};
pub use menu::{Menu, MenuBar, MenuEntry, MenuError, MenuRole};
pub use message::{MappedAppCx, MessageMapper};
#[cfg(target_arch = "wasm32")]
pub use runner::spawn_on_canvas;
pub use runner::{
    ActiveTaskSnapshot, App, AppBackend, AppConfig, AppCx, Clipboard, CloseResponse, FixedStep,
    FontDatabaseOptions, Instant, MainResult, MessageKey, MessageProxy, Monitor, ProxyClosed,
    RunError, RuntimeConfig, RuntimePolicy, RuntimeSnapshot, TaskCompletion, TaskCompletionStatus,
    TaskConfig, TaskError, TaskId, TaskKind, TaskSpawnError, Theme, TimerId, Ui, UpdateInfo,
    WindowAttributes, WindowConfig, WindowEvent, WindowId,
};
#[doc(hidden)]
pub use runner::{TaskAbandon, TaskMessageFactory, TaskSink, TaskSubmit};
#[cfg(not(target_arch = "wasm32"))]
pub use runner::{run, run_with};
pub use state::{
    JsonStateStore, PersistError, StateEnvelope, WindowPlacement, WindowPlacementTracker,
};
pub use subscription::{
    ActiveSubscriptionSnapshot, DeliveryPolicy, Subscription, SubscriptionConfig, SubscriptionId,
    SubscriptionKind, SubscriptionStatus, Subscriptions,
};
#[doc(hidden)]
pub use subscription::{
    RawSubscriptionEvent, RawSubscriptionSink, ServiceSubscriptionFactory,
    ServiceSubscriptionStart, SubscriptionEventSink, SubscriptionFactory,
};
pub use undo::{UndoAction, UndoStack, redo_command_id, sync_undo_commands, undo_command_id};

/// Backend-neutral GPU types used by application-owned render views.
pub use astrelis_gpu as gpu;
