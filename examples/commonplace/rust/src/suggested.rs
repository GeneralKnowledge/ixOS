//! Suggested feeds: a short, categorised catalogue of popular feeds that
//! the Add Feed sheet offers. It is data, kept in
//! `catalogue/suggested-feeds.json` so it can be edited without touching
//! code, and built into the app. Suggestions are only offered: nothing is
//! subscribed until the person taps one (NetNewsWire lesson 2,
//! docs/research/experience/references/netnewswire.md).

use serde::Deserialize;

const CATALOGUE: &str = include_str!("../catalogue/suggested-feeds.json");

/// One category of suggestions, such as News or Science.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SuggestedCategory {
    pub name: String,
    pub feeds: Vec<SuggestedFeed>,
}

/// A feed the person can subscribe to with one tap.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct SuggestedFeed {
    /// A stable slug, for the row's accessibility identifier.
    pub id: String,
    pub title: String,
    /// The feed's address, passed to `add_feed` as it is.
    pub url: String,
    /// One short line on what it is.
    pub about: String,
    /// The person already subscribes to this address (written any way).
    pub subscribed: bool,
}

#[derive(Deserialize)]
struct Catalogue {
    categories: Vec<CatalogueCategory>,
}

#[derive(Deserialize)]
struct CatalogueCategory {
    name: String,
    feeds: Vec<CatalogueFeed>,
}

#[derive(Deserialize)]
struct CatalogueFeed {
    id: String,
    title: String,
    url: String,
    about: String,
}

/// The catalogue, with `subscribed` set by `is_subscribed`. Empty, and
/// logged, if the built-in file doesn't parse (a test keeps it parsing).
pub(crate) fn suggestions(is_subscribed: impl Fn(&str) -> bool) -> Vec<SuggestedCategory> {
    let catalogue: Catalogue = match serde_json::from_str(CATALOGUE) {
        Ok(catalogue) => catalogue,
        Err(e) => {
            forge_runtime::logging::error!("the suggested-feeds catalogue doesn't parse: {e}");
            return Vec::new();
        }
    };
    catalogue
        .categories
        .into_iter()
        .map(|category| SuggestedCategory {
            name: category.name,
            feeds: category
                .feeds
                .into_iter()
                .map(|feed| SuggestedFeed {
                    subscribed: is_subscribed(&feed.url),
                    id: feed.id,
                    title: feed.title,
                    url: feed.url,
                    about: feed.about,
                })
                .collect(),
        })
        .collect()
}
