//! The `From` conversions every app error writes for the framework's own
//! errors, written once (app-friction-log A15).

/// Writes `From` conversions from the framework's errors into an app's
/// error enum, so `?` works on runtime, document and storage calls.
///
/// The app's error needs the cases the conversions it asks for produce:
///
/// | Source | Becomes |
/// |---|---|
/// | [`Stopped`](crate::Stopped) | `Stopped { kind }` |
/// | [`OpenError`](crate::OpenError) | `InUse` when another core holds the data directory, else `Storage { reason }` |
/// | [`DocumentError`](crate::DocumentError) | `Corrupt { reason }`, `NewerData` or `Storage { reason }` |
/// | `forge_storage::StorageError` | `Corrupt { reason }`, `NewerData` or `Storage { reason }` |
///
/// With no list it writes the conversions for `Stopped`, `OpenError` and
/// `DocumentError`, the three every [`Document`](crate::Document) app needs.
/// An app whose error has other shapes (Pocket Library's `NewerLibrary`,
/// Commonplace's `Cancelled`) writes its own `From` instead, and
/// `FileStoreError` stays the app's because what a file failure means is the
/// app's to say.
///
/// ```ignore
/// #[derive(Debug, thiserror::Error, uniffi::Error)]
/// pub enum NotesError {
///     #[error("the title is empty")]
///     EmptyTitle,
///     #[error("Notes is open in another copy of the app")]
///     InUse,
///     #[error("the saved notes can't be read: {reason}")]
///     Corrupt { reason: String },
///     #[error("the notes were saved by a newer version of the app")]
///     NewerData,
///     #[error("storage error: {reason}")]
///     Storage { reason: String },
///     #[error("stopped ({kind:?})")]
///     Stopped { kind: forge_app::ErrorKind },
/// }
///
/// forge_app::app_error_conversions!(NotesError);
/// // or, for a SQLite app: (NotesError: Stopped, OpenError, StorageError)
/// ```
#[macro_export]
macro_rules! app_error_conversions {
    ($error:ty) => {
        $crate::app_error_conversions!($error: Stopped, OpenError, DocumentError);
    };
    ($error:ty: $($source:ident),+ $(,)?) => {
        $( $crate::__app_error_from!($error, $source); )+
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __app_error_from {
    ($error:ty, Stopped) => {
        impl ::std::convert::From<$crate::Stopped> for $error {
            fn from(stopped: $crate::Stopped) -> Self {
                Self::Stopped { kind: stopped.kind }
            }
        }
    };
    ($error:ty, OpenError) => {
        impl ::std::convert::From<$crate::OpenError> for $error {
            fn from(err: $crate::OpenError) -> Self {
                match err {
                    $crate::OpenError::InUse { .. } => Self::InUse,
                    other => Self::Storage {
                        reason: ::std::string::ToString::to_string(&other),
                    },
                }
            }
        }
    };
    ($error:ty, DocumentError) => {
        impl ::std::convert::From<$crate::DocumentError> for $error {
            fn from(err: $crate::DocumentError) -> Self {
                match err {
                    $crate::DocumentError::Unreadable { reason } => Self::Corrupt { reason },
                    $crate::DocumentError::Newer { .. } => Self::NewerData,
                    $crate::DocumentError::Io { reason } => Self::Storage { reason },
                }
            }
        }
    };
    ($error:ty, StorageError) => {
        impl ::std::convert::From<$crate::__storage::StorageError> for $error {
            fn from(err: $crate::__storage::StorageError) -> Self {
                match err {
                    $crate::__storage::StorageError::CorruptData { reason } => {
                        Self::Corrupt { reason }
                    }
                    $crate::__storage::StorageError::UnsupportedSchemaVersion { .. } => {
                        Self::NewerData
                    }
                    other => Self::Storage {
                        reason: ::std::string::ToString::to_string(&other),
                    },
                }
            }
        }
    };
    ($error:ty, $other:ident) => {
        ::std::compile_error!(concat!(
            "app_error_conversions! knows Stopped, OpenError, DocumentError and StorageError, not ",
            stringify!($other)
        ));
    };
}

#[cfg(test)]
mod tests {
    use crate::{DocumentError, ErrorKind, OpenError, Stopped};
    use forge_storage::StorageError;

    #[derive(Debug, PartialEq)]
    enum AppError {
        InUse,
        Corrupt { reason: String },
        NewerData,
        Storage { reason: String },
        Stopped { kind: ErrorKind },
    }

    crate::app_error_conversions!(AppError: Stopped, OpenError, DocumentError, StorageError);

    #[test]
    fn stopped_keeps_its_kind() {
        let err: AppError = Stopped {
            kind: ErrorKind::Interrupted,
        }
        .into();
        assert_eq!(
            err,
            AppError::Stopped {
                kind: ErrorKind::Interrupted
            }
        );
    }

    #[test]
    fn open_error_in_use_is_in_use_and_io_is_storage() {
        let err: AppError = OpenError::InUse { path: "x".into() }.into();
        assert_eq!(err, AppError::InUse);
        let err: AppError = OpenError::Io {
            reason: "disk".into(),
        }
        .into();
        assert!(matches!(err, AppError::Storage { reason } if reason.contains("disk")));
    }

    #[test]
    fn document_errors_map_to_corrupt_newer_and_storage() {
        let err: AppError = DocumentError::Unreadable {
            reason: "bad".into(),
        }
        .into();
        assert_eq!(
            err,
            AppError::Corrupt {
                reason: "bad".into()
            }
        );
        let err: AppError = DocumentError::Newer {
            found: 3,
            supported: 2,
        }
        .into();
        assert_eq!(err, AppError::NewerData);
        let err: AppError = DocumentError::Io {
            reason: "full".into(),
        }
        .into();
        assert_eq!(
            err,
            AppError::Storage {
                reason: "full".into()
            }
        );
    }

    #[test]
    fn storage_errors_map_to_corrupt_newer_and_storage() {
        let err: AppError = StorageError::corrupt("bad").into();
        assert_eq!(
            err,
            AppError::Corrupt {
                reason: "bad".into()
            }
        );
        let err: AppError = StorageError::UnsupportedSchemaVersion {
            found: 3,
            supported: 2,
        }
        .into();
        assert_eq!(err, AppError::NewerData);
        let err: AppError = StorageError::MissingDirectory.into();
        assert!(matches!(err, AppError::Storage { .. }));
    }

    #[test]
    fn the_default_list_is_the_document_three() {
        #[derive(Debug, PartialEq)]
        enum DocError {
            InUse,
            Corrupt { reason: String },
            NewerData,
            Storage { reason: String },
            Stopped { kind: ErrorKind },
        }
        crate::app_error_conversions!(DocError);
        let err: DocError = OpenError::InUse { path: "x".into() }.into();
        assert_eq!(err, DocError::InUse);
        let _ = DocError::Corrupt {
            reason: String::new(),
        };
        let _ = DocError::NewerData;
        let _ = DocError::Storage {
            reason: String::new(),
        };
        let _ = DocError::Stopped {
            kind: ErrorKind::Cancelled,
        };
    }
}
