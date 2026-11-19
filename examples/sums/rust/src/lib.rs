//! Sums: a calculator rebuilt from the outside, the Calculator expedition's
//! recreation (docs/research/app-as-specification.md).
//!
//! Every rule is in [`engine`], and comes from the expedition's evidence
//! (`docs/research/expeditions/calculator/`): the findings register and the
//! answers recorded from the reference on an iPhone. Where the evidence ran
//! out the rule is a guess, marked `GUESS:` and listed in the reconstruction
//! record. The words people see ("Undefined") are the reference's, kept here
//! because the test compares them exactly; Swift decides how they look.
//!
//! This file is the API the SwiftUI app calls, on the application runtime
//! (`forge_app`): each key press is a command, run one at a time on the
//! runtime's command thread. The keys pressed since the last All Clear are
//! saved, so the sum on the screen survives the app being closed, as the
//! reference's does (R0 finding 21).

pub mod engine;

use std::sync::Arc;

use forge_app::{DataDirectory, ErrorKind, OpenError, Published, Runtime, Stopped};
use forge_storage::{JsonFile, StorageError};
use serde::{Deserialize, Serialize};

pub use engine::{format_result, Calculator, Key};

uniffi::setup_scaffolding!();

/// Version of `sums.json`. Bump it (and migrate) when [`Saved`] changes shape.
const SAVE_VERSION: u32 = 1;
const SAVE_FILE: &str = "sums.json";

/// A key, as Swift sees it.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SumsKey {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Decimal,
    Add,
    Subtract,
    Multiply,
    Divide,
    Equals,
    Percent,
    Negate,
    Delete,
    Clear,
}

impl From<SumsKey> for Key {
    fn from(key: SumsKey) -> Key {
        match key {
            SumsKey::Zero => Key::Digit(0),
            SumsKey::One => Key::Digit(1),
            SumsKey::Two => Key::Digit(2),
            SumsKey::Three => Key::Digit(3),
            SumsKey::Four => Key::Digit(4),
            SumsKey::Five => Key::Digit(5),
            SumsKey::Six => Key::Digit(6),
            SumsKey::Seven => Key::Digit(7),
            SumsKey::Eight => Key::Digit(8),
            SumsKey::Nine => Key::Digit(9),
            SumsKey::Decimal => Key::Decimal,
            SumsKey::Add => Key::Add,
            SumsKey::Subtract => Key::Subtract,
            SumsKey::Multiply => Key::Multiply,
            SumsKey::Divide => Key::Divide,
            SumsKey::Equals => Key::Equals,
            SumsKey::Percent => Key::Percent,
            SumsKey::Negate => Key::Negate,
            SumsKey::Delete => Key::Delete,
            SumsKey::Clear => Key::Clear,
        }
    }
}

/// What the screen shows.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SumsScreen {
    /// The number (or "Undefined") in the big line.
    pub display: String,
    /// The sum above it, after equals ("12+7").
    pub expression: Option<String>,
    /// True while the clear key clears only the entry ("C"); false when it
    /// clears everything ("AC").
    pub clears_entry: bool,
}

fn screen(calc: &Calculator) -> SumsScreen {
    SumsScreen {
        display: calc.display(),
        expression: calc.expression(),
        clears_entry: calc.clears_entry(),
    }
}

/// What can go wrong. Arrives in Swift as a thrown `SumsError`.
#[derive(uniffi::Error, thiserror::Error, Clone, Debug, PartialEq, Eq)]
pub enum SumsError {
    /// The saved sum could not be read or saved.
    #[error("storage error: {reason}")]
    Storage { reason: String },
    /// Another copy of the app has the data open (one core per data folder).
    #[error("the calculator is open in another copy of the app")]
    InUse,
    /// The app is shutting down and takes no more key presses.
    #[error("the app has been shut down")]
    ShutDown,
}

impl From<Stopped> for SumsError {
    fn from(_: Stopped) -> Self {
        Self::ShutDown
    }
}

impl From<StorageError> for SumsError {
    fn from(err: StorageError) -> Self {
        Self::Storage {
            reason: err.to_string(),
        }
    }
}

impl From<OpenError> for SumsError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::InUse { .. } => Self::InUse,
            other => Self::Storage {
                reason: other.to_string(),
            },
        }
    }
}

/// The framework kind of an error, so ForgeKit can present any of them.
#[uniffi::export]
pub fn sums_error_kind(error: SumsError) -> ErrorKind {
    match error {
        SumsError::Storage { .. } => ErrorKind::Internal,
        SumsError::InUse => ErrorKind::Unavailable,
        SumsError::ShutDown => ErrorKind::ShutDown,
    }
}

/// What is written to `sums.json`: the reference ids of the keys pressed
/// since the calculator was last fully cleared. Opening replays them, so the
/// saved state is exactly what the rules make of those keys.
#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    keys: Vec<String>,
}

/// What the command thread owns.
struct State {
    calc: Calculator,
    saved: Saved,
    file: JsonFile<Saved>,
    published: Arc<Published<SumsScreen>>,
}

/// The app. ForgeKit's `ForgeHost` opens one per process and keeps it.
#[derive(uniffi::Object)]
pub struct SumsCore {
    rt: Runtime<State>,
    screen: Arc<Published<SumsScreen>>,
}

#[uniffi::export]
impl SumsCore {
    /// Opens the calculator saved in `storage_directory`, or a cleared one.
    /// Fails with `InUse` if another core has that folder open.
    #[uniffi::constructor]
    pub fn new(storage_directory: String) -> Result<Arc<Self>, SumsError> {
        forge_runtime::logging::init();
        if storage_directory.is_empty() {
            return Err(SumsError::Storage {
                reason: "storage directory must not be empty".into(),
            });
        }
        let directory = DataDirectory::open(&storage_directory)?;
        let file = JsonFile::new(directory.path().join(SAVE_FILE), SAVE_VERSION);
        let saved: Saved = file.load()?.unwrap_or_default();
        let mut calc = Calculator::new();
        for id in &saved.keys {
            // An id this version doesn't know is skipped, not fatal.
            if let Some(key) = Key::from_id(id) {
                calc.press(key);
            }
        }
        let published = Arc::new(Published::new(screen(&calc)));
        let state = State {
            calc,
            saved,
            file,
            published: Arc::clone(&published),
        };
        Ok(Arc::new(Self {
            rt: Runtime::with_directory("sums-core", directory, state),
            screen: published,
        }))
    }

    /// What the screen shows now. Quick: it never waits for a command.
    pub fn screen(&self) -> SumsScreen {
        self.screen.get().as_ref().clone()
    }

    /// Presses a key, saves, and returns what the screen shows after it.
    pub async fn press(&self, key: SumsKey) -> Result<SumsScreen, SumsError> {
        self.rt
            .command("press", move |state| {
                let key = Key::from(key);
                let mut calc = state.calc.clone();
                calc.press(key);
                let mut next = state.saved.clone();
                if key == Key::Clear && calc == Calculator::new() {
                    // All Clear: nothing before it matters any more.
                    next.keys.clear();
                } else {
                    next.keys.push(key.id().to_string());
                }
                state.file.save(&next)?;
                state.calc = calc;
                state.saved = next;
                let shown = screen(&state.calc);
                state.published.set(shown.clone());
                Ok(shown)
            })
            .await
    }
}

// events(), set_phase(), phase(), revision() and shutdown(), for ForgeKit.
forge_app::export_runtime!(SumsCore, rt);

#[cfg(test)]
mod tests {
    use super::*;
    use forge_runtime::block_on;

    fn open(dir: &tempfile::TempDir) -> Arc<SumsCore> {
        SumsCore::new(dir.path().to_string_lossy().into_owned()).unwrap()
    }

    fn press(core: &SumsCore, keys: &[SumsKey]) -> SumsScreen {
        let mut last = core.screen();
        for key in keys {
            last = block_on(core.press(*key)).unwrap();
        }
        last
    }

    #[test]
    fn a_sum_survives_a_restart() {
        use SumsKey::*;
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        let shown = press(&core, &[One, Two, Add, Seven, Equals]);
        assert_eq!(shown.display, "19");
        assert_eq!(shown.expression.as_deref(), Some("12+7"));
        drop(core);

        let reopened = open(&dir);
        assert_eq!(reopened.screen(), shown);
        // Equals still repeats the last step after the restart.
        assert_eq!(press(&reopened, &[Equals]).display, "26");
    }

    #[test]
    fn all_clear_forgets_the_keys() {
        use SumsKey::*;
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        let shown = press(&core, &[Four, Add, Two, Equals, Clear]);
        assert_eq!(shown.display, "0");
        assert!(!shown.clears_entry);
        drop(core);
        let text = std::fs::read_to_string(dir.path().join(SAVE_FILE)).unwrap();
        assert!(!text.contains("Four"), "{text}");
    }

    #[test]
    fn one_core_per_folder_and_nothing_after_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let core = open(&dir);
        assert_eq!(
            SumsCore::new(dir.path().to_string_lossy().into_owned()).err(),
            Some(SumsError::InUse)
        );
        core.shutdown();
        assert_eq!(block_on(core.press(SumsKey::One)), Err(SumsError::ShutDown));
    }
}
