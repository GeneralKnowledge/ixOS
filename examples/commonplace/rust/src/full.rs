//! Full articles (slice C4): the readable part of an article's own web
//! page, for feeds that send only a summary.
//!
//! The page is fetched like a feed (forge-net, URLSession on iOS) and its
//! main text found by `dom_smoothie`, a Rust port of Mozilla's
//! Readability. What comes back is HTML that ForgeReader lays out like a
//! feed's own text; nothing in the page runs.

use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, Request};

use crate::model::AppError;

/// The largest page read. Pages bigger than this are almost never an
/// article.
pub const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;
/// How long one page may take to answer.
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
/// Elements read from one page, so a huge page can't take long to score.
const MAX_ELEMENTS: usize = 20_000;
/// Less text than this is no article (a menu, a sign-in prompt): the
/// feed's own text is kept instead.
const MIN_TEXT_CHARS: usize = 200;

/// Fetches the page at `url` and returns its article as HTML.
pub fn fetch_article(
    http: &dyn HttpClient,
    url: &str,
    cancel: &CancelHandle,
) -> Result<String, AppError> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::NoReadableText {
            reason: "not a web address".into(),
        });
    }
    let request = Request::get(url)
        .timeout(PAGE_TIMEOUT)
        .header(
            "Accept",
            "text/html, application/xhtml+xml;q=0.9, */*;q=0.1",
        )
        .header("User-Agent", "Commonplace/0.1 (ixOS)");
    let response = http.send_cancellable(&request, cancel)?;
    if !response.is_success() {
        return Err(AppError::Http {
            status: response.status,
        });
    }
    if let Some(kind) = response.header("Content-Type") {
        let kind = kind.to_ascii_lowercase();
        if !(kind.contains("html") || kind.contains("xml") || kind.starts_with("text/")) {
            return Err(AppError::NoReadableText {
                reason: "not a web page".into(),
            });
        }
    }
    if response.body.len() > MAX_PAGE_BYTES {
        return Err(AppError::NoReadableText {
            reason: "page too large".into(),
        });
    }
    extract(&String::from_utf8_lossy(&response.body), url)
}

/// The article in a page's HTML, as HTML. `url` resolves its relative
/// links and pictures.
pub fn extract(page: &str, url: &str) -> Result<String, AppError> {
    let config = dom_smoothie::Config {
        max_elements_to_parse: MAX_ELEMENTS,
        ..Default::default()
    };
    let mut readability =
        dom_smoothie::Readability::new(page, Some(url), Some(config)).map_err(|e| {
            AppError::NoReadableText {
                reason: e.to_string(),
            }
        })?;
    let article = readability.parse().map_err(|e| AppError::NoReadableText {
        reason: e.to_string(),
    })?;
    let html = article.content.to_string();
    if article.text_content.trim().chars().count() < MIN_TEXT_CHARS {
        return Err(AppError::NoReadableText {
            reason: "too little text".into(),
        });
    }
    Ok(html)
}
