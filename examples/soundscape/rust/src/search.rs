//! One search for everything: the library, Jamendo and the Internet
//! Archive at once.
//!
//! The library answers at once, on the phone (`library::search`). The two
//! catalogues are asked together, each on its own thread, and each answers
//! for itself: one that fails or isn't set up says why in its own section
//! and never hides the other. A catalogue track already in the library is
//! left out of the catalogue's results, since the library shows it
//! (with the person's edits and any download). Each catalogue shows its
//! first few results; Discover has the rest.

use forge_net::{CancelHandle, HttpClient};

use crate::archive::{self, ArchiveItem};
use crate::catalogue::CatalogueTrack;
use crate::jamendo;
use crate::model::SoundscapeError;

/// The most results shown from each catalogue.
pub const MAX_PER_SOURCE: usize = 8;

/// What the catalogues found for one search.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct CatalogueSearch {
    /// Jamendo tracks not already in the library, best first.
    pub jamendo: Vec<CatalogueTrack>,
    /// Why Jamendo has no results to show, if it couldn't be asked or
    /// failed ("add your Jamendo client ID in Discover first").
    pub jamendo_problem: Option<String>,
    /// Internet Archive items (albums, concerts), most downloaded first.
    pub archive: Vec<ArchiveItem>,
    pub archive_problem: Option<String>,
}

/// Asks both catalogues for `query` at once. An empty query asks nothing
/// (the catalogues' popular lists belong to Discover). A cancelled search
/// is an error; any other failure is that catalogue's problem.
pub fn search_catalogues(
    http: &dyn HttpClient,
    jamendo_client_id: Option<&str>,
    query: &str,
    in_library: &dyn Fn(&str) -> bool,
    cancel: &CancelHandle,
) -> Result<CatalogueSearch, SoundscapeError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(CatalogueSearch::default());
    }
    let (jamendo, archive) = std::thread::scope(|scope| {
        let jamendo =
            scope.spawn(|| jamendo_client_id.map(|id| jamendo::search(http, id, query, cancel)));
        let archive = archive::search(http, query, cancel);
        (
            jamendo.join().unwrap_or_else(|_| {
                Some(Err(SoundscapeError::Server {
                    reason: "the Jamendo search stopped unexpectedly".into(),
                }))
            }),
            archive,
        )
    });
    if cancel.is_cancelled() {
        return Err(stopped());
    }
    let mut found = CatalogueSearch::default();
    match jamendo {
        None => {
            found.jamendo_problem =
                Some("add your Jamendo client ID in Discover to search it".into())
        }
        Some(Ok(tracks)) => {
            found.jamendo = tracks
                .into_iter()
                .filter(|t| !in_library(&t.id))
                .take(MAX_PER_SOURCE)
                .collect()
        }
        Some(Err(err)) => found.jamendo_problem = Some(problem(err)?),
    }
    match archive {
        Ok(items) => found.archive = items.into_iter().take(MAX_PER_SOURCE).collect(),
        Err(err) => found.archive_problem = Some(problem(err)?),
    }
    Ok(found)
}

fn stopped() -> SoundscapeError {
    SoundscapeError::Stopped {
        kind: forge_app::ErrorKind::Cancelled,
    }
}

/// A catalogue's failure in words; a stop stays an error.
fn problem(err: SoundscapeError) -> Result<String, SoundscapeError> {
    match err {
        SoundscapeError::Stopped { .. } => Err(err),
        other => Ok(other.to_string()),
    }
}
