//! Opaque application error type used by RXUI application callbacks.

use std::fmt;

/// Boxed error for RXUI application callbacks.
///
/// Any error implementing [`std::error::Error`] converts into `Error` via
/// `?`, so application code can mix `UiError`, `HostError`, `io::Error`,
/// and persistence errors without `map_err` glue. Like `anyhow::Error`,
/// this type deliberately does not implement [`std::error::Error`] itself;
/// that is what makes the blanket [`From`] conversion possible.
pub struct Error(Box<dyn std::error::Error + Send + Sync + 'static>);

impl Error {
    /// Creates an error from a display-able message.
    pub fn msg(message: impl fmt::Display) -> Self {
        Self(message.to_string().into())
    }

    /// Returns the underlying error if it is of type `E`.
    pub fn downcast_ref<E: std::error::Error + 'static>(&self) -> Option<&E> {
        self.0.downcast_ref()
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(cause) = source {
            write!(f, "\n  caused by: {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

/// Result alias used throughout RXUI application code.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Adapter that fills `astrelis_app::App::Error`, which requires
/// [`std::error::Error`].
pub(crate) struct DynAppError(pub(crate) Error);

impl fmt::Display for DynAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for DynAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for DynAppError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn converts_std_errors_via_question_mark() {
        fn fails() -> Result<()> {
            Err(io::Error::other("disk on fire"))?;
            Ok(())
        }
        let error = fails().unwrap_err();
        assert_eq!(error.to_string(), "disk on fire");
        assert!(error.downcast_ref::<io::Error>().is_some());
    }

    #[test]
    fn msg_builds_ad_hoc_errors() {
        let error = Error::msg(format_args!("bad {}", "state"));
        assert_eq!(error.to_string(), "bad state");
        assert!(error.downcast_ref::<io::Error>().is_none());
    }

    #[test]
    fn debug_renders_source_chain() {
        #[derive(Debug)]
        struct Outer(io::Error);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "outer failed")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let error = Error::from(Outer(io::Error::other("inner cause")));
        let rendered = format!("{error:?}");
        assert!(rendered.contains("outer failed"));
        assert!(rendered.contains("caused by: inner cause"));
    }
}
