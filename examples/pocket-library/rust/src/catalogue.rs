//! The online book catalogue. [`BookCatalogue`] is what the library needs;
//! [`OpenLibrary`] is the one implementation, over any `forge_net` client, so
//! tests run it against canned responses and another service could replace it.
//!
//! The catalogue is optional: every failure here is a typed error, and
//! nothing in the library changes until an import succeeds.

use std::time::Duration;

use forge_net::{HttpClient, Request, Response};
use serde::Deserialize;

use crate::epub::image_extension;
use crate::error::LibraryError;

/// A search result before the library marks which ones it already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueHit {
    pub key: String,
    pub title: String,
    pub author: String,
    pub first_published: Option<i32>,
    pub total_pages: Option<u32>,
    /// A small cover for result lists.
    pub cover_url: Option<String>,
    /// A larger cover to save with the book.
    pub large_cover_url: Option<String>,
}

pub trait BookCatalogue: Send + Sync {
    /// Up to 20 works matching `query`.
    fn search(&self, query: &str) -> Result<Vec<CatalogueHit>, LibraryError>;
    /// The work's description, if the catalogue has one.
    fn description(&self, key: &str) -> Result<Option<String>, LibraryError>;
    /// Downloads a cover image. Returns the bytes and a file extension.
    fn cover(&self, url: &str) -> Result<(Vec<u8>, &'static str), LibraryError>;
}

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
const COVER_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESULTS: usize = 20;
const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;

/// [Open Library](https://openlibrary.org/developers/api): free, no API key.
pub struct OpenLibrary<C> {
    http: C,
    api_base: String,
    covers_base: String,
}

impl<C: HttpClient> OpenLibrary<C> {
    pub fn new(http: C) -> Self {
        Self::with_bases(
            http,
            "https://openlibrary.org",
            "https://covers.openlibrary.org",
        )
    }

    pub fn with_bases(http: C, api_base: &str, covers_base: &str) -> Self {
        Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            covers_base: covers_base.trim_end_matches('/').to_string(),
        }
    }

    pub fn search_url(&self, query: &str) -> String {
        format!(
            "{}/search.json?q={}&fields=key,title,author_name,first_publish_year,cover_i,number_of_pages_median&limit={MAX_RESULTS}",
            self.api_base,
            percent_encode(query)
        )
    }

    fn get(&self, url: String, timeout: Duration) -> Result<Response, LibraryError> {
        let request = Request::get(url)
            .header("Accept", "application/json")
            .timeout(timeout);
        let response = self.http.send(&request)?;
        check_status(&response)?;
        Ok(response)
    }

    fn cover_url(&self, id: i64, size: char) -> String {
        format!("{}/b/id/{id}-{size}.jpg", self.covers_base)
    }
}

/// Maps HTTP failures to typed errors. 5xx and 429 mean "try later"; other
/// non-2xx answers mean we asked for something the service does not have.
fn check_status(response: &Response) -> Result<(), LibraryError> {
    match response.status {
        200..=299 => Ok(()),
        429 => Err(LibraryError::CatalogueUnavailable {
            reason: "too many requests (HTTP 429)".into(),
        }),
        500..=599 => Err(LibraryError::CatalogueUnavailable {
            reason: format!("server error (HTTP {})", response.status),
        }),
        status => Err(LibraryError::BadResponse {
            reason: format!("HTTP {status}"),
        }),
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    docs: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SearchDoc {
    key: String,
    title: String,
    #[serde(default)]
    author_name: Vec<String>,
    first_publish_year: Option<i32>,
    cover_i: Option<i64>,
    number_of_pages_median: Option<u32>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Description {
    Text(String),
    Typed { value: String },
}

#[derive(Deserialize)]
struct Work {
    description: Option<Description>,
}

/// Work keys go into URLs, so only the expected shape is accepted.
fn valid_work_key(key: &str) -> bool {
    key.strip_prefix("/works/")
        .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
}

impl<C: HttpClient> BookCatalogue for OpenLibrary<C> {
    fn search(&self, query: &str) -> Result<Vec<CatalogueHit>, LibraryError> {
        let response = self.get(self.search_url(query), SEARCH_TIMEOUT)?;
        parse_search(&response.body, |id, size| self.cover_url(id, size))
    }

    fn description(&self, key: &str) -> Result<Option<String>, LibraryError> {
        if !valid_work_key(key) {
            return Err(LibraryError::invalid(format!(
                "not an Open Library work key: {key:?}"
            )));
        }
        let response = self.get(format!("{}{key}.json", self.api_base), SEARCH_TIMEOUT)?;
        let work: Work =
            serde_json::from_slice(&response.body).map_err(|e| LibraryError::BadResponse {
                reason: format!("work: {e}"),
            })?;
        Ok(work
            .description
            .map(|d| match d {
                Description::Text(text) | Description::Typed { value: text } => text,
            })
            .map(|text| clean_description(&text))
            .filter(|text| !text.is_empty()))
    }

    fn cover(&self, url: &str) -> Result<(Vec<u8>, &'static str), LibraryError> {
        if !url.starts_with(&format!("{}/", self.covers_base)) {
            return Err(LibraryError::invalid(format!(
                "not a catalogue cover: {url}"
            )));
        }
        // `default=false` makes a missing cover a 404 instead of a blank image.
        let response = self.get(format!("{url}?default=false"), COVER_TIMEOUT)?;
        if response.body.len() > MAX_COVER_BYTES {
            return Err(LibraryError::BadResponse {
                reason: "cover image is too large".into(),
            });
        }
        let extension =
            image_extension(&response.body).ok_or_else(|| LibraryError::BadResponse {
                reason: "cover is not an image".into(),
            })?;
        Ok((response.body, extension))
    }
}

/// Parses `search.json`. A response that is not the expected JSON is
/// `BadResponse`; individual results without a key or title are skipped,
/// unless every result is unusable, which is also `BadResponse`.
pub fn parse_search(
    body: &[u8],
    cover_url: impl Fn(i64, char) -> String,
) -> Result<Vec<CatalogueHit>, LibraryError> {
    let response: SearchResponse =
        serde_json::from_slice(body).map_err(|e| LibraryError::BadResponse {
            reason: format!("search: {e}"),
        })?;
    let total = response.docs.len();
    let hits: Vec<CatalogueHit> = response
        .docs
        .into_iter()
        .filter_map(|doc| serde_json::from_value::<SearchDoc>(doc).ok())
        .filter(|doc| valid_work_key(&doc.key) && !doc.title.trim().is_empty())
        .map(|doc| CatalogueHit {
            author: match doc.author_name.as_slice() {
                [] => String::new(),
                [one] => one.clone(),
                [first, second] => format!("{first}, {second}"),
                [first, second, ..] => format!("{first}, {second} and others"),
            },
            key: doc.key,
            title: doc.title.trim().to_string(),
            first_published: doc.first_publish_year,
            total_pages: doc.number_of_pages_median.filter(|p| *p > 0),
            cover_url: doc
                .cover_i
                .filter(|id| *id > 0)
                .map(|id| cover_url(id, 'M')),
            large_cover_url: doc
                .cover_i
                .filter(|id| *id > 0)
                .map(|id| cover_url(id, 'L')),
        })
        .take(MAX_RESULTS)
        .collect();
    if total > 0 && hits.is_empty() {
        return Err(LibraryError::BadResponse {
            reason: "no usable results in the response".into(),
        });
    }
    Ok(hits)
}

/// Open Library descriptions often end with a "([source][1])" footnote and
/// Markdown link definitions; keep the prose.
fn clean_description(text: &str) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .take_while(|line| !line.trim_start().starts_with("----------"))
        .filter(|line| {
            let line = line.trim_start();
            !(line.starts_with('[') && line.contains("]: http"))
        })
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

/// Percent-encodes a query parameter value (RFC 3986 unreserved kept).
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use forge_net::{Method, NetError, StaticClient};

    pub(crate) const API: &str = "https://catalogue.test";
    pub(crate) const COVERS: &str = "https://covers.test";

    /// The shape of a real `search.json` answer (fields trimmed to the ones
    /// requested), including the awkward cases: no author, no cover, a
    /// result without a title, and many authors.
    pub(crate) const SEARCH_BODY: &str = r#"{
      "numFound": 4, "start": 0, "numFoundExact": true,
      "docs": [
        {"key": "/works/OL27482W", "title": "The Hobbit", "author_name": ["J.R.R. Tolkien"],
         "first_publish_year": 1937, "cover_i": 14627509, "number_of_pages_median": 310},
        {"key": "/works/OL1W", "title": "Anonymous Pamphlet"},
        {"key": "/works/OL2W"},
        {"key": "/works/OL3W", "title": "Collected Essays", "author_name": ["A", "B", "C"],
         "cover_i": -1, "number_of_pages_median": 0}
      ],
      "q": "hobbit"
    }"#;

    pub(crate) fn catalogue(client: StaticClient) -> OpenLibrary<StaticClient> {
        OpenLibrary::with_bases(client, API, COVERS)
    }

    fn ok(body: &str) -> Result<Response, NetError> {
        Ok(Response::new(200, body.as_bytes().to_vec()))
    }

    #[test]
    fn parses_search_results_and_skips_unusable_ones() {
        let ol = catalogue(StaticClient::new());
        let url = ol.search_url("hobbit");
        let ol = catalogue(StaticClient::new().route(Method::Get, url, ok(SEARCH_BODY)));
        let hits = ol.search("hobbit").unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0],
            CatalogueHit {
                key: "/works/OL27482W".into(),
                title: "The Hobbit".into(),
                author: "J.R.R. Tolkien".into(),
                first_published: Some(1937),
                total_pages: Some(310),
                cover_url: Some(format!("{COVERS}/b/id/14627509-M.jpg")),
                large_cover_url: Some(format!("{COVERS}/b/id/14627509-L.jpg")),
            }
        );
        assert_eq!(hits[1].author, "");
        assert_eq!(hits[1].cover_url, None);
        assert_eq!(hits[2].author, "A, B and others");
        assert_eq!(hits[2].total_pages, None);
        assert_eq!(hits[2].cover_url, None);
    }

    #[test]
    fn queries_are_encoded() {
        let ol = catalogue(StaticClient::new());
        assert!(ol
            .search_url("Œuvres & co/1")
            .contains("q=%C5%92uvres+%26+co%2F1&"));
    }

    #[test]
    fn malformed_and_failed_responses_are_typed() {
        let ol = catalogue(StaticClient::new());
        let url = ol.search_url("x");
        let with = |result| catalogue(StaticClient::new().route(Method::Get, url.clone(), result));

        // A maintenance page instead of JSON.
        let err = with(ok("<html>Down for maintenance</html>"))
            .search("x")
            .unwrap_err();
        assert!(matches!(err, LibraryError::BadResponse { .. }), "{err:?}");
        // JSON of the wrong shape.
        let err = with(ok(r#"{"error": "nope"}"#)).search("x").unwrap_err();
        assert!(matches!(err, LibraryError::BadResponse { .. }), "{err:?}");
        // Every result unusable.
        let err = with(ok(r#"{"docs": [{"title": 3}]}"#))
            .search("x")
            .unwrap_err();
        assert!(matches!(err, LibraryError::BadResponse { .. }), "{err:?}");
        // No results is fine.
        assert_eq!(with(ok(r#"{"docs": []}"#)).search("x").unwrap(), vec![]);

        let status = |code| Ok(Response::new(code, Vec::new()));
        assert!(matches!(
            with(status(503)).search("x"),
            Err(LibraryError::CatalogueUnavailable { .. })
        ));
        assert!(matches!(
            with(status(429)).search("x"),
            Err(LibraryError::CatalogueUnavailable { .. })
        ));
        assert!(matches!(
            with(status(404)).search("x"),
            Err(LibraryError::BadResponse { .. })
        ));
        assert_eq!(
            with(Err(NetError::Timeout)).search("x"),
            Err(LibraryError::Timeout)
        );
        assert!(matches!(
            with(Err(NetError::Offline {
                reason: "no route".into()
            }))
            .search("x"),
            Err(LibraryError::Offline { .. })
        ));
        assert!(matches!(
            with(Err(NetError::Certificate {
                reason: "expired".into()
            }))
            .search("x"),
            Err(LibraryError::Certificate { .. })
        ));
    }

    #[test]
    fn descriptions_come_in_two_shapes_and_lose_their_footnotes() {
        let plain = format!("{API}/works/OL1W.json");
        let typed = format!("{API}/works/OL2W.json");
        let none = format!("{API}/works/OL3W.json");
        let ol = catalogue(
            StaticClient::new()
                .route(Method::Get, plain, ok(r#"{"description": "Plain text."}"#))
                .route(
                    Method::Get,
                    typed,
                    ok(r#"{"description": {"type": "/type/text", "value": "A hobbit goes on an adventure.\r\n\r\n----------\r\nAlso contained in:\r\n[Other](https://openlibrary.org/x)"}}"#),
                )
                .route(Method::Get, none, ok(r#"{"title": "No description"}"#)),
        );
        assert_eq!(
            ol.description("/works/OL1W").unwrap().as_deref(),
            Some("Plain text.")
        );
        assert_eq!(
            ol.description("/works/OL2W").unwrap().as_deref(),
            Some("A hobbit goes on an adventure.")
        );
        assert_eq!(ol.description("/works/OL3W").unwrap(), None);
        // Keys are never pasted into URLs unchecked.
        assert!(matches!(
            ol.description("/works/../../admin"),
            Err(LibraryError::InvalidInput { .. })
        ));
    }

    #[test]
    fn covers_must_be_images_from_the_cover_service() {
        let jpeg = format!("{COVERS}/b/id/1-L.jpg");
        let html = format!("{COVERS}/b/id/2-L.jpg");
        let ol = catalogue(
            StaticClient::new()
                .route(
                    Method::Get,
                    format!("{jpeg}?default=false"),
                    Ok(Response::new(200, vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2])),
                )
                .route(Method::Get, format!("{html}?default=false"), ok("<html/>")),
        );
        assert_eq!(ol.cover(&jpeg).unwrap().1, "jpg");
        assert!(matches!(
            ol.cover(&html),
            Err(LibraryError::BadResponse { .. })
        ));
        assert!(matches!(
            ol.cover(&format!("{COVERS}/b/id/3-L.jpg")),
            Err(LibraryError::BadResponse { .. }) // 404: no such cover
        ));
        assert!(matches!(
            ol.cover("https://evil.test/x.jpg"),
            Err(LibraryError::InvalidInput { .. })
        ));
    }
}
