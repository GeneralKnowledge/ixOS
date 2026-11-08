//! Exploration aimed at a change (docs/forge-m6.md, M6.2).
//!
//! The explorer is given a [`Focus`]: the accessibility identifiers that the
//! changed source files declare (`direct`), the identifiers of the views that
//! present or call into them (`entry`), routes that earlier walks found to
//! screens holding them, and earlier issues to check again. With a focus it
//! tries focus controls first wherever it is, follows the known routes before
//! wandering, prefers screens with focus controls left, and reports what it
//! actually exercised of the focus and what it never reached.
//!
//! Which identifiers a change touches is worked out from the source by
//! `forge ui focus` (forge-cli): this module only uses the result. Every
//! link carries the file it came from and how, so a finding can name the
//! changed file it is linked to, or say there is no link.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::explore::{Action, Issue, Report};
use crate::{Locator, UiNode};

/// How strongly a control is tied to the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    /// Not part of the focus.
    None,
    /// Its identifier's screen part (`record` of `record.save`) is a focus
    /// screen's, by the `screen.element` naming rule (AGENTS.md).
    Screen,
    /// Declared in a file that uses a type or function the change declares:
    /// a way into the changed code (a list row that opens a changed view).
    Entry,
    /// Declared in a changed file.
    Direct,
}

/// One identifier (or identifier prefix, from `"record.result.\(key)"`)
/// tied to a changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// An identifier, or a prefix ending in `.` when `prefix` is set.
    pub id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prefix: bool,
    pub strength: Strength,
    /// The changed file (repository-relative) it leads back to.
    pub file: String,
    /// How: `declared in ExerciseView.swift`, `TodayView.swift presents
    /// ExerciseView`, `GoodDogModel.swift calls recordOutcome (training.rs)`.
    pub how: String,
}

/// A way to a screen, from an earlier walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// `focus_screen` (an earlier walk saw focus identifiers there) or
    /// `recheck` (an earlier issue's replay).
    pub kind: String,
    /// The earlier screen, or the earlier issue (`issue 3: app_closed`).
    pub target: String,
    /// The actions from launch.
    pub actions: Vec<Action>,
}

/// What the walk is aimed at.
#[derive(Debug, Clone, Default)]
pub struct Focus {
    pub links: Vec<Link>,
    /// Routes to follow before wandering, in order.
    pub routes: Vec<Route>,
}

impl Focus {
    pub fn is_empty(&self) -> bool {
        self.links.is_empty() && self.routes.is_empty()
    }

    /// The strongest tie between `id` and the change.
    pub fn strength(&self, id: &str) -> Strength {
        let mut best = Strength::None;
        for link in &self.links {
            let hit = if link.prefix {
                id.starts_with(&link.id)
            } else {
                link.id == id
            };
            if hit {
                best = best.max(link.strength);
            }
        }
        if best == Strength::None {
            let screen = screen_part(id);
            if !screen.is_empty()
                && self
                    .links
                    .iter()
                    .any(|l| l.strength == Strength::Direct && screen_part(&l.id) == screen)
            {
                best = Strength::Screen;
            }
        }
        best
    }

    /// The changed files `id` is tied to, and how.
    pub fn files_for(&self, id: &str) -> Vec<Value> {
        let mut out = Vec::new();
        for link in &self.links {
            let hit = if link.prefix {
                id.starts_with(&link.id)
            } else {
                link.id == id
            };
            if hit {
                out.push(json!({"file": link.file, "how": link.how, "via": id, "strength": link.strength}));
            }
        }
        out
    }

    /// Direct and entry identifiers in a tree: does this screen hold any?
    pub fn ids_in(&self, tree: &UiNode) -> BTreeSet<String> {
        tree.walk()
            .filter_map(|n| n.identifier.as_deref())
            .filter(|id| self.strength(id) >= Strength::Entry)
            .map(str::to_owned)
            .collect()
    }

    /// Every direct identifier (prefixes end in `*`), for coverage.
    pub fn direct_targets(&self) -> BTreeSet<String> {
        self.links
            .iter()
            .filter(|l| l.strength == Strength::Direct)
            .map(|l| {
                if l.prefix {
                    format!("{}*", l.id)
                } else {
                    l.id.clone()
                }
            })
            .collect()
    }
}

/// `record` of `record.save`; empty for an identifier without a dot.
pub fn screen_part(id: &str) -> &str {
    id.split_once('.').map(|(s, _)| s).unwrap_or("")
}

/// The identifier an action targets, if it targets one.
pub fn action_id(action: &Action) -> Option<&str> {
    match action {
        Action::Tap(Locator::Id(id)) | Action::Type(Locator::Id(id), _) => Some(id),
        _ => None,
    }
}

/// Routes from an earlier walk's `explore.json`: to every screen where it
/// saw an identifier the change declares, shortest first, and the
/// replays of its issues worth checking again (errors, and warnings on
/// focus screens), at most `max` in all.
pub fn routes_from(previous: &Value, focus: &Focus, max: usize) -> Vec<Route> {
    let mut routes = Vec::new();
    let mut screens: Vec<&Value> = previous["screens"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    screens.sort_by_key(|s| s["route"].as_array().map_or(usize::MAX, Vec::len));
    for issue in previous["issues"].as_array().into_iter().flatten() {
        let error = issue["severity"] == "error";
        let on_focus = issue["focus"].as_str().is_some_and(|f| f != "none");
        if !(error || (on_focus && issue["severity"] == "warning")) {
            continue;
        }
        if let Some(actions) = issue["route"].as_array().and_then(|r| parse_route(r).ok()) {
            routes.push(Route {
                kind: "recheck".into(),
                target: format!(
                    "issue {}: {} ({})",
                    issue["number"],
                    issue["issue"].as_str().unwrap_or(""),
                    issue["message"].as_str().unwrap_or("")
                ),
                actions,
            });
        }
    }
    for screen in screens {
        let ids: Vec<&str> = screen["identifiers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|id| focus.strength(id) == Strength::Direct)
            .collect();
        if ids.is_empty() {
            continue;
        }
        let Some(actions) = screen["route"].as_array().and_then(|r| parse_route(r).ok()) else {
            continue;
        };
        // The launch screen needs no route.
        if actions.is_empty() {
            continue;
        }
        routes.push(Route {
            kind: "focus_screen".into(),
            target: format!(
                "screen {} (holds {})",
                screen["id"].as_str().unwrap_or(""),
                ids.join(", ")
            ),
            actions,
        });
    }
    let mut seen = BTreeSet::new();
    routes.retain(|r| seen.insert(format!("{:?}", r.actions)));
    routes.truncate(max);
    routes
}

/// Actions written as `forge ui run` steps (what `route` holds in
/// `explore.json`): `tap = { id = "x" }`, `type = { id = "x", text = "y" }`.
pub fn parse_route(steps: &[Value]) -> Result<Vec<Action>, String> {
    let mut text = String::new();
    for step in steps {
        let step = step.as_str().ok_or("a route step is not text")?;
        text.push_str("[[step]]\n");
        text.push_str(step);
        text.push('\n');
    }
    #[derive(Deserialize)]
    struct Steps {
        #[serde(default)]
        step: Vec<crate::script::Step>,
    }
    let parsed: Steps = toml::from_str(&text).map_err(|e| e.to_string())?;
    parsed
        .step
        .into_iter()
        .filter(|s| s.launch.is_none())
        .map(|s| match (s.tap, s.type_text) {
            (Some(target), None) => Ok(Action::Tap(target.locator()?)),
            (None, Some(t)) => Ok(Action::Type(t.target.locator()?, t.text)),
            _ => Err("a route step is a tap or a type".to_string()),
        })
        .collect()
}

/// Issues of this walk against an earlier one's. An earlier issue is
/// `gone` only when this walk did what showed it (the same action from the
/// same place, or its replay) and it didn't come back; otherwise it is
/// `not_rechecked`. The same kind with the same message counts as the same
/// issue.
pub fn compare(previous: &Value, previous_path: &str, now: &Report) -> Value {
    let key = |kind: &str, message: &str| format!("{kind}\u{0}{message}");
    let now_keys: BTreeSet<String> = now
        .issues
        .iter()
        .map(|i| key(&i.issue, &i.message))
        .collect();
    let done: BTreeSet<String> = now.journal.iter().map(|s| s.action.clone()).collect();
    let mut still = Vec::new();
    let mut gone = Vec::new();
    let mut unchecked = Vec::new();
    let mut before = BTreeSet::new();
    for issue in previous["issues"].as_array().into_iter().flatten() {
        let kind = issue["issue"].as_str().unwrap_or("");
        let message = issue["message"].as_str().unwrap_or("");
        before.insert(key(kind, message));
        let summary = json!({"number": issue["number"], "issue": kind, "message": message, "severity": issue["severity"]});
        if now_keys.contains(&key(kind, message)) {
            still.push(summary);
            continue;
        }
        // What showed it: the last action of its replay.
        let last = issue["repro"]
            .as_array()
            .and_then(|r| r.last())
            .and_then(Value::as_str);
        let rechecked = match last {
            Some(action) => done.contains(action),
            // Found on arrival (an audit issue): its screen was seen again.
            None => now
                .screens
                .iter()
                .any(|s| Some(s.id.as_str()) == issue["screen"].as_str()),
        };
        if rechecked {
            gone.push(summary);
        } else {
            unchecked.push(summary);
        }
    }
    let new: Vec<Value> = now
        .issues
        .iter()
        .filter(|i| !before.contains(&key(&i.issue, &i.message)))
        .map(|i| json!({"number": i.number, "issue": i.issue, "message": i.message, "severity": i.severity}))
        .collect();
    json!({
        "previous": previous_path,
        "previous_seed": previous["seed"],
        "new": new,
        "still_present": still,
        "gone": gone,
        "not_rechecked": unchecked,
    })
}

/// How sure an issue is, from what showed it: `observed` (the phone showed
/// it: the app closed, an alert, a missing label, a control off screen) or
/// `heuristic` (inferred: a tap that changed nothing may be a toggle with no
/// visible state; a dead end may be closed by a gesture).
pub fn confidence(issue: &Issue) -> &'static str {
    match issue.issue.as_str() {
        "no_effect" | "dead_end" => "heuristic",
        _ => "observed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, strength: Strength) -> Link {
        Link {
            id: id.trim_end_matches('*').into(),
            prefix: id.ends_with('*'),
            strength,
            file: "examples/good-dog/Sources/AppHost/ExerciseView.swift".into(),
            how: "declared in ExerciseView.swift".into(),
        }
    }

    #[test]
    fn strength_by_identifier_prefix_and_screen() {
        let focus = Focus {
            links: vec![
                link("record.save", Strength::Direct),
                link("record.result.*", Strength::Direct),
                link("today.activity.*", Strength::Entry),
            ],
            routes: vec![],
        };
        assert_eq!(focus.strength("record.save"), Strength::Direct);
        assert_eq!(focus.strength("record.result.easy"), Strength::Direct);
        assert_eq!(focus.strength("today.activity.0"), Strength::Entry);
        // Same screen part as a direct identifier, by the naming rule.
        assert_eq!(focus.strength("record.cancel"), Strength::Screen);
        assert_eq!(focus.strength("tab.dog"), Strength::None);
        assert_eq!(focus.strength("BackButton"), Strength::None);
        assert_eq!(focus.files_for("record.result.easy").len(), 1);
        assert!(focus.files_for("tab.dog").is_empty());
    }

    #[test]
    fn routes_parse_back_from_their_steps() {
        let steps = vec![
            json!(r#"tap = { id = "today.activity.0" }"#),
            json!(r#"type = { id = "dogForm.name", text = "Moss" }"#),
            json!(r#"tap = { label = "Done" }"#),
        ];
        let actions = parse_route(&steps).unwrap();
        assert_eq!(
            actions,
            vec![
                Action::Tap(Locator::Id("today.activity.0".into())),
                Action::Type(Locator::Id("dogForm.name".into()), "Moss".into()),
                Action::Tap(Locator::Label("Done".into())),
            ]
        );
        assert!(parse_route(&[json!("screenshot = \"x\"")]).is_err());
    }
}
