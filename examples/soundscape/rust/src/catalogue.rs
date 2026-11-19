//! Tracks from free music catalogues (Jamendo first; see `docs/sources.md`).
//!
//! A catalogue track keeps its rights as separate facts: whether it may be
//! streamed, downloaded for offline listening, or adapted, and the credit it
//! needs. Downloadable never means reusable. The rights are saved with the
//! track when it is added to the library, so they stay after the catalogue
//! changes or can't be reached.

use serde::{Deserialize, Serialize};

/// What a catalogue track's licence and its provider allow.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rights {
    /// Short name, such as `CC BY-NC-SA 3.0`.
    pub licence: String,
    pub licence_url: Option<String>,
    /// The credit to show: title, artist, licence and where it came from.
    pub attribution: String,
    pub may_stream: bool,
    /// May be kept on the phone for offline listening.
    pub may_download: bool,
    /// May be changed or used in something new (no "ND" in its licence).
    pub may_adapt: bool,
    /// Only for non-commercial use ("NC" in its licence).
    pub non_commercial: bool,
    /// The track's page on the catalogue's site.
    pub page_url: Option<String>,
}

/// One track from a catalogue, as a search returns it and as the library
/// keeps it once added.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CatalogueTrack {
    /// `jamendo:<id>`: stable, so the same track is recognised again.
    pub id: String,
    /// The catalogue's name, such as `Jamendo`.
    pub provider: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub number: Option<u32>,
    pub genre: Option<String>,
    pub duration_seconds: Option<f64>,
    pub artwork_url: Option<String>,
    /// What AVPlayer streams.
    pub audio_url: String,
    pub rights: Rights,
    /// When it was added to the library (0 in search results).
    #[serde(default)]
    pub added_ms: i64,
}

/// Rights from a Creative Commons licence URL
/// (`http://creativecommons.org/licenses/by-nc-sa/3.0/`) and the provider's
/// own download permission. An unknown licence allows streaming only (the
/// catalogue serves it), never adapting, and downloading only as the
/// provider says.
pub fn rights_from_licence(
    licence_url: Option<&str>,
    provider_allows_download: bool,
    credit: &str,
    page_url: Option<String>,
) -> Rights {
    let url = licence_url.map(str::trim).filter(|u| !u.is_empty());
    let (licence, may_adapt, non_commercial) = match url.and_then(parse_cc) {
        Some(cc) => cc,
        None => ("Licence not stated".to_string(), false, false),
    };
    Rights {
        attribution: format!("{credit}, {licence}"),
        licence,
        licence_url: url.map(str::to_string),
        may_stream: true,
        may_download: provider_allows_download,
        may_adapt,
        non_commercial,
        page_url,
    }
}

/// (name, may adapt, non-commercial) from a Creative Commons URL.
fn parse_cc(url: &str) -> Option<(String, bool, bool)> {
    let lower = url.to_lowercase();
    if !lower.contains("creativecommons.org/") {
        return None;
    }
    let after = |marker: &str| {
        lower
            .split_once(marker)
            .map(|(_, rest)| rest.trim_matches('/').to_string())
    };
    if let Some(rest) = after("/publicdomain/") {
        let mut parts = rest.split('/');
        let kind = parts.next().unwrap_or("");
        let version = parts.next().unwrap_or("");
        let name = match kind {
            "zero" => "CC0",
            "mark" => "Public Domain",
            _ => return None,
        };
        return Some((join(name, version), true, false));
    }
    let rest = after("/licenses/")?;
    let mut parts = rest.split('/');
    let terms = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("");
    let known = ["by", "nc", "sa", "nd"];
    let list: Vec<&str> = terms.split('-').collect();
    if list.first() != Some(&"by") || !list.iter().all(|t| known.contains(t)) {
        return None;
    }
    let name = format!("CC {}", terms.to_uppercase());
    Some((
        join(&name, version),
        !list.contains(&"nd"),
        list.contains(&"nc"),
    ))
}

fn join(name: &str, version: &str) -> String {
    if version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {version}")
    }
}

/// The provider of a catalogue track id, or `None` for the person's own
/// music.
pub fn provider_of(id: &str) -> Option<&'static str> {
    if id.starts_with(crate::jamendo::PREFIX) {
        Some(crate::jamendo::PROVIDER)
    } else if id.starts_with(crate::archive::PREFIX) {
        Some(crate::archive::PROVIDER)
    } else {
        None
    }
}
