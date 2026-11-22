//! "Organise my feeds": a typed proposal to sort subscriptions into
//! folders, the check that turns it into moves, and (in `lib.rs`) the apply
//! step that runs them through `create_folder` and `move_feed`.
//!
//! This is Commonplace's own capability, with nothing of A2UI in it
//! (docs/research/app-capability-audit.md, 4.2). Device Lab's A2UI Lab draws
//! a surface for it; App Intents or an agent over Relay could use the same
//! types later.
//!
//! A proposal only refers to feeds and folders the library already has,
//! plus at most [`MAX_NEW_FOLDERS`] new folder names. Applying re-checks it
//! against the library as it is then, so a plan made from an older snapshot
//! is refused rather than applied to feeds that have moved since.

use serde::{Deserialize, Serialize};

use crate::{suggested, Feed, Folder};

/// The most feeds one plan may move (and a surface shows).
pub const MAX_FEEDS: usize = 30;
/// The most folders a surface offers as targets.
pub const MAX_FOLDERS: usize = 8;
/// The most new folders one plan may create.
pub const MAX_NEW_FOLDERS: usize = 3;
/// The longest folder name a plan may create, in characters.
pub const MAX_NAME: usize = 40;

/// What the library holds, as far as organising goes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OrganiseSnapshot {
    pub feeds: Vec<OrganiseFeed>,
    pub folders: Vec<OrganiseFolder>,
    /// Names of top-level folders the snapshot doesn't offer (past the
    /// first [`MAX_FOLDERS`]), so a new folder can't take one of them.
    #[serde(default)]
    pub other_names: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OrganiseFeed {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub folder_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OrganiseFolder {
    pub id: i64,
    /// Its path from the top ("Tech / News"), so two folders of the same
    /// name in different places can be told apart.
    pub name: String,
}

/// Separates a nested folder's name from its parents' in a path.
pub const PATH_SEPARATOR: &str = " / ";

/// `folder`'s path from the top, through its parents in `all`.
fn path(folder: &Folder, all: &[Folder]) -> String {
    let mut parts = vec![folder.name.as_str()];
    let mut parent = folder.parent_id;
    // Bounded by the folder count, so a damaged parent loop can't spin.
    for _ in 0..all.len() {
        let Some(p) = parent.and_then(|id| all.iter().find(|f| f.id == id)) else {
            break;
        };
        parts.push(p.name.as_str());
        parent = p.parent_id;
    }
    parts.reverse();
    parts.join(PATH_SEPARATOR)
}

/// Where a feed goes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Folder { id: i64 },
    NewFolder { name: String },
    NoFolder,
}

/// One feed's move, with the folder it is in now, so a plan made from an
/// older snapshot can be told apart.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FeedMove {
    pub feed_id: i64,
    pub from: Option<i64>,
    pub to: Target,
}

/// A proposal: what a model (or the stand-in [`propose`]) fills.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct FolderPlan {
    pub moves: Vec<FeedMove>,
}

/// Why a plan was refused. Names fields and limits, never a feed's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganiseError(pub String);

fn err(text: impl Into<String>) -> OrganiseError {
    OrganiseError(text.into())
}

/// The library's feeds and folders, as a plan may use them: the first
/// [`MAX_FOLDERS`] folders, and the first [`MAX_FEEDS`] feeds that are in
/// no folder or in one of those.
pub fn snapshot(feeds: &[Feed], all: &[Folder]) -> OrganiseSnapshot {
    let folders: Vec<OrganiseFolder> = all
        .iter()
        .take(MAX_FOLDERS)
        .map(|f| OrganiseFolder {
            id: f.id,
            name: path(f, all),
        })
        .collect();
    let other_names = all
        .iter()
        .skip(MAX_FOLDERS)
        .filter(|f| f.parent_id.is_none())
        .map(|f| f.name.clone())
        .collect();
    let feeds = feeds
        .iter()
        .filter(|f| {
            f.folder_id
                .is_none_or(|id| folders.iter().any(|d| d.id == id))
        })
        .take(MAX_FEEDS)
        .map(|f| OrganiseFeed {
            id: f.id,
            title: f.title.clone(),
            url: f.url.clone(),
            folder_id: f.folder_id,
        })
        .collect();
    OrganiseSnapshot {
        feeds,
        folders,
        other_names,
    }
}

/// A clean folder name, or why not.
pub fn clean_name(name: &str) -> Result<String, OrganiseError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err("A new folder needs a name."));
    }
    if name.chars().count() > MAX_NAME {
        return Err(err(format!(
            "Folder names are at most {MAX_NAME} characters."
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(err("A folder name can't hold control characters."));
    }
    Ok(name.to_string())
}

impl OrganiseSnapshot {
    pub fn feed(&self, id: i64) -> Option<&OrganiseFeed> {
        self.feeds.iter().find(|f| f.id == id)
    }

    pub fn folder(&self, id: i64) -> Option<&OrganiseFolder> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// The folder named `name` (ignoring case), if there is one.
    pub fn folder_named(&self, name: &str) -> Option<&OrganiseFolder> {
        self.folders
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name.trim()))
    }

    /// A folder's name for the change set: "No folder" for none.
    pub fn place(&self, folder_id: Option<i64>) -> String {
        match folder_id.and_then(|id| self.folder(id)) {
            Some(folder) => folder.name.clone(),
            None => "No folder".into(),
        }
    }
}

/// Checks `plan` against `snapshot`: known feeds, each once, each starting
/// where the snapshot has it; known target folders; at most
/// [`MAX_NEW_FOLDERS`] clean new names, none the name of an existing folder.
/// Returns the moves that change something, new names cleaned.
pub fn check(
    snapshot: &OrganiseSnapshot,
    plan: &FolderPlan,
) -> Result<Vec<FeedMove>, OrganiseError> {
    if plan.moves.len() > MAX_FEEDS {
        return Err(err(format!("A plan moves at most {MAX_FEEDS} feeds.")));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut new_names: Vec<String> = Vec::new();
    let mut moves = Vec::new();
    for m in &plan.moves {
        let feed = snapshot
            .feed(m.feed_id)
            .ok_or_else(|| err(format!("Unknown feed {}.", m.feed_id)))?;
        if !seen.insert(m.feed_id) {
            return Err(err(format!("Feed {} appears twice.", m.feed_id)));
        }
        if feed.folder_id != m.from {
            return Err(err("A feed has moved since the plan was made."));
        }
        let to = match &m.to {
            Target::Folder { id } => {
                snapshot
                    .folder(*id)
                    .ok_or_else(|| err(format!("Unknown folder {id}.")))?;
                Target::Folder { id: *id }
            }
            Target::NewFolder { name } => {
                let name = clean_name(name)?;
                let other = snapshot
                    .other_names
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(&name));
                if other || snapshot.folder_named(&name).is_some() {
                    return Err(err("A new folder has the name of an existing one."));
                }
                if !new_names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    new_names.push(name.clone());
                }
                Target::NewFolder { name }
            }
            Target::NoFolder => Target::NoFolder,
        };
        let unchanged = match &to {
            Target::Folder { id } => feed.folder_id == Some(*id),
            Target::NoFolder => feed.folder_id.is_none(),
            Target::NewFolder { .. } => false,
        };
        if !unchanged {
            moves.push(FeedMove {
                feed_id: m.feed_id,
                from: m.from,
                to,
            });
        }
    }
    if new_names.len() > MAX_NEW_FOLDERS {
        return Err(err(format!(
            "A plan creates at most {MAX_NEW_FOLDERS} new folders."
        )));
    }
    Ok(moves)
}

/// The stand-in for a model (as A2UI-1's fixed proposal was): every feed
/// in no folder whose address is in the suggested-feeds catalogue goes to
/// the folder named after its category, which is made if the library has
/// none. Deterministic, and the same shape a model would fill.
pub fn propose(snapshot: &OrganiseSnapshot) -> FolderPlan {
    let categories = suggested::suggestions(|_| false);
    let mut new_names: Vec<String> = Vec::new();
    let mut moves = Vec::new();
    for feed in snapshot.feeds.iter().filter(|f| f.folder_id.is_none()) {
        let Some(category) = categories.iter().find(|c| {
            c.feeds
                .iter()
                .any(|s| crate::same_address(&s.url, &feed.url))
        }) else {
            continue;
        };
        let to = match snapshot.folder_named(&category.name) {
            Some(folder) => Target::Folder { id: folder.id },
            None => {
                let known = new_names.iter().any(|n| n == &category.name);
                if !known && new_names.len() >= MAX_NEW_FOLDERS {
                    continue;
                }
                if !known {
                    new_names.push(category.name.clone());
                }
                Target::NewFolder {
                    name: category.name.clone(),
                }
            }
        };
        moves.push(FeedMove {
            feed_id: feed.id,
            from: None,
            to,
        });
    }
    FolderPlan { moves }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> OrganiseSnapshot {
        OrganiseSnapshot {
            feeds: vec![
                OrganiseFeed {
                    id: 1,
                    title: "One".into(),
                    url: "https://a.example/feed".into(),
                    folder_id: None,
                },
                OrganiseFeed {
                    id: 2,
                    title: "Two".into(),
                    url: "https://b.example/feed".into(),
                    folder_id: Some(10),
                },
            ],
            folders: vec![OrganiseFolder {
                id: 10,
                name: "Reading".into(),
            }],
            other_names: vec!["Archive".into()],
        }
    }

    fn folder(id: i64, parent_id: Option<i64>, name: &str) -> Folder {
        Folder {
            id,
            parent_id,
            name: name.into(),
            depth: u32::from(parent_id.is_some()),
            feed_count: 0,
            unread_count: 0,
        }
    }

    #[test]
    fn nested_folders_show_their_path_and_hidden_names_stay_taken() {
        let mut all = vec![folder(1, None, "Tech"), folder(2, Some(1), "News")];
        all.extend((3..=9).map(|id| folder(id, None, &format!("F{id}"))));
        all.push(folder(10, None, "Later"));
        let s = snapshot(&[], &all);
        assert_eq!(s.folders.len(), MAX_FOLDERS);
        assert_eq!(s.folder(2).unwrap().name, "Tech / News");
        assert_eq!(s.other_names, vec!["F9".to_string(), "Later".to_string()]);
    }

    #[test]
    fn a_new_folder_cant_take_a_hidden_folders_name() {
        let plan = FolderPlan {
            moves: vec![mv(
                1,
                None,
                Target::NewFolder {
                    name: "archive".into(),
                },
            )],
        };
        assert!(check(&snap(), &plan).is_err());
    }

    fn mv(feed_id: i64, from: Option<i64>, to: Target) -> FeedMove {
        FeedMove { feed_id, from, to }
    }

    #[test]
    fn a_valid_plan_keeps_only_real_moves() {
        let plan = FolderPlan {
            moves: vec![
                mv(
                    1,
                    None,
                    Target::NewFolder {
                        name: "  Tech ".into(),
                    },
                ),
                mv(2, Some(10), Target::Folder { id: 10 }),
            ],
        };
        let moves = check(&snap(), &plan).unwrap();
        assert_eq!(
            moves,
            vec![mv(
                1,
                None,
                Target::NewFolder {
                    name: "Tech".into()
                }
            )]
        );
    }

    #[test]
    fn refusals() {
        let cases = [
            mv(9, None, Target::NoFolder),
            mv(1, Some(10), Target::NoFolder),
            mv(1, None, Target::Folder { id: 99 }),
            mv(1, None, Target::NewFolder { name: " ".into() }),
            mv(
                1,
                None,
                Target::NewFolder {
                    name: "reading".into(),
                },
            ),
            mv(
                1,
                None,
                Target::NewFolder {
                    name: "x".repeat(41),
                },
            ),
            mv(
                1,
                None,
                Target::NewFolder {
                    name: "a\nb".into(),
                },
            ),
        ];
        for case in cases {
            let plan = FolderPlan {
                moves: vec![case.clone()],
            };
            assert!(check(&snap(), &plan).is_err(), "{case:?}");
        }
        let twice = FolderPlan {
            moves: vec![mv(1, None, Target::NoFolder), mv(1, None, Target::NoFolder)],
        };
        assert!(check(&snap(), &twice).is_err());
    }

    #[test]
    fn at_most_three_new_folders() {
        let mut s = snap();
        for id in 3..=6 {
            s.feeds.push(OrganiseFeed {
                id,
                title: format!("F{id}"),
                url: format!("https://{id}.example/feed"),
                folder_id: None,
            });
        }
        let plan = FolderPlan {
            moves: (3..=6)
                .map(|id| {
                    mv(
                        id,
                        None,
                        Target::NewFolder {
                            name: format!("N{id}"),
                        },
                    )
                })
                .collect(),
        };
        assert!(check(&s, &plan).is_err());
    }

    #[test]
    fn the_stand_in_proposal_passes_its_own_check() {
        let categories = suggested::suggestions(|_| false);
        let first = &categories[0];
        let mut s = snap();
        s.feeds[0].url = first.feeds[0].url.clone();
        let plan = propose(&s);
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(
            plan.moves[0].to,
            Target::NewFolder {
                name: first.name.clone()
            }
        );
        assert_eq!(check(&s, &plan).unwrap().len(), 1);
        // With a folder of that name, the feed goes there instead.
        s.folders.push(OrganiseFolder {
            id: 11,
            name: first.name.to_uppercase(),
        });
        assert_eq!(propose(&s).moves[0].to, Target::Folder { id: 11 });
    }

    #[test]
    fn snapshot_leaves_out_feeds_in_folders_it_doesnt_offer() {
        let folders: Vec<Folder> = (0..10)
            .map(|i| Folder {
                id: i,
                parent_id: None,
                name: format!("F{i}"),
                depth: 0,
                feed_count: 0,
                unread_count: 0,
            })
            .collect();
        let s = snapshot(&[], &folders);
        assert_eq!(s.folders.len(), MAX_FOLDERS);
    }
}
