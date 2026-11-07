//! The capability ledger as data: one TOML record per table row under
//! `docs/ledger/<table>/`, rendered into `docs/capability-ledger.md`.
//!
//! `docs/ledger/page.md` is the page with each table replaced by a marker line
//! `<!-- ledger: <kind> <table> -->`. Parallel PRs add or edit separate record
//! files instead of the same Markdown table lines, so they stop conflicting.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const OUTPUT: &str = "docs/capability-ledger.md";
const MARKER_PREFIX: &str = "<!-- ledger: ";
const MARKER_SUFFIX: &str = " -->";

/// A kind of table: its record fields in column order, and its header cells.
struct Kind {
    name: &'static str,
    fields: &'static [&'static str],
    headers: &'static [&'static str],
}

const KINDS: &[Kind] = &[
    Kind {
        name: "module",
        fields: &[
            "module",
            "layer",
            "crate",
            "provides",
            "native_apis",
            "dependencies",
            "ios",
            "android",
            "desktop",
            "status",
            "exercised_by",
        ],
        headers: &[
            "Module",
            "Layer",
            "Crate and feature",
            "Provides",
            "Native APIs",
            "Dependencies",
            "iOS",
            "Android",
            "Desktop",
            "Status",
            "Exercised by",
        ],
    },
    Kind {
        name: "capability",
        fields: &[
            "capability",
            "framework",
            "rust_api",
            "ios_native",
            "in_app",
            "iphone",
            "documented",
            "limits",
        ],
        headers: &[
            "Capability",
            "Framework",
            "Rust API",
            "iOS native",
            "In an app",
            "iPhone",
            "Documented",
            "Limits",
        ],
    },
    Kind {
        name: "metric",
        fields: &[
            "metric",
            "target",
            "how_measured",
            "first_needed_by",
            "last_recorded",
        ],
        headers: &[
            "Metric",
            "Proposed target",
            "How measured",
            "First needed by",
            "Last recorded",
        ],
    },
    Kind {
        name: "dx_step",
        fields: &["step", "docs", "last_run", "time_taken", "findings"],
        headers: &[
            "Step",
            "Docs they start from",
            "Last run",
            "Time taken",
            "Findings",
        ],
    },
];

fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

/// One row: its sort key, the file it came from and its cells in column order.
#[derive(Debug)]
pub struct Record {
    pub order: i64,
    pub file: PathBuf,
    pub cells: Vec<String>,
}

fn parse_record(kind: &Kind, path: &Path, text: &str) -> Result<Record, String> {
    let name = path.display();
    let table: toml::Table = text.parse().map_err(|e| format!("{name}: {e}"))?;
    let order = match table.get("order") {
        Some(toml::Value::Integer(n)) => *n,
        Some(_) => return Err(format!("{name}: `order` must be an integer")),
        None => return Err(format!("{name}: missing `order` (rows sort by it)")),
    };
    for key in table.keys() {
        if key != "order" && !kind.fields.contains(&key.as_str()) {
            return Err(format!(
                "{name}: unknown field `{key}`; a {} record has: order, {}",
                kind.name,
                kind.fields.join(", ")
            ));
        }
    }
    let mut cells = Vec::with_capacity(kind.fields.len());
    for field in kind.fields {
        let cell = match table.get(*field) {
            None => String::new(),
            Some(toml::Value::String(s)) => s.clone(),
            Some(_) => return Err(format!("{name}: `{field}` must be a string")),
        };
        if cell.contains('|') || cell.contains('\n') {
            return Err(format!(
                "{name}: `{field}` contains `|` or a line break, which would break the table"
            ));
        }
        cells.push(cell);
    }
    if cells[0].trim().is_empty() {
        return Err(format!("{name}: `{}` must not be empty", kind.fields[0]));
    }
    Ok(Record {
        order,
        file: path.to_path_buf(),
        cells,
    })
}

/// The records in `dir`, sorted by `order` then file name.
fn load_table(kind: &Kind, dir: &Path) -> Result<Vec<Record>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut records = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        records.push(parse_record(kind, &path, &text)?);
    }
    records.sort_by(|a, b| a.order.cmp(&b.order).then(a.file.cmp(&b.file)));
    Ok(records)
}

fn row(cells: &[String]) -> String {
    let mut line = String::from("|");
    for cell in cells {
        if cell.is_empty() {
            line.push_str(" |");
        } else {
            line.push(' ');
            line.push_str(cell);
            line.push_str(" |");
        }
    }
    line
}

fn render_table(kind: &Kind, records: &[Record]) -> String {
    let headers: Vec<String> = kind.headers.iter().map(|h| h.to_string()).collect();
    let mut lines = vec![row(&headers), format!("|{}", "---|".repeat(headers.len()))];
    lines.extend(records.iter().map(|r| row(&r.cells)));
    lines.join("\n")
}

/// A marker line's kind and table directory.
fn marker(line: &str) -> Option<(&str, &str)> {
    let inner = line
        .trim()
        .strip_prefix(MARKER_PREFIX)?
        .strip_suffix(MARKER_SUFFIX)?;
    let mut parts = inner.split_whitespace();
    let (kind, table) = (parts.next()?, parts.next()?);
    parts.next().is_none().then_some((kind, table))
}

/// Renders the page from `ledger_dir` (which holds `page.md` and the table
/// directories).
pub fn render(ledger_dir: &Path) -> Result<String, String> {
    let template_path = ledger_dir.join("page.md");
    let template = fs::read_to_string(&template_path)
        .map_err(|e| format!("cannot read {}: {e}", template_path.display()))?;
    let mut out = Vec::new();
    for line in template.split('\n') {
        match marker(line) {
            Some((kind_name, table)) => {
                let kind = kind(kind_name).ok_or_else(|| {
                    format!(
                        "{}: unknown table kind `{kind_name}`",
                        template_path.display()
                    )
                })?;
                let records = load_table(kind, &ledger_dir.join(table))?;
                out.push(render_table(kind, &records));
            }
            None if line.trim_start().starts_with(MARKER_PREFIX) => {
                return Err(format!(
                    "{}: malformed marker `{line}`; expected `<!-- ledger: <kind> <table> -->`",
                    template_path.display()
                ));
            }
            None => out.push(line.to_string()),
        }
    }
    Ok(out.join("\n"))
}

/// Every table in the page: (kind, table directory) in page order.
fn tables(ledger_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let template_path = ledger_dir.join("page.md");
    let template = fs::read_to_string(&template_path)
        .map_err(|e| format!("cannot read {}: {e}", template_path.display()))?;
    Ok(template
        .lines()
        .filter_map(marker)
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .collect())
}

/// `forge ledger generate`: writes the page from the records.
pub fn generate(root: &Path) -> Result<(), String> {
    let page = render(&root.join("docs/ledger"))?;
    let out = root.join(OUTPUT);
    fs::write(&out, page).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {OUTPUT} from docs/ledger");
    Ok(())
}

/// `forge ledger check`: fails when the page differs from the records.
pub fn check(root: &Path) -> Result<(), String> {
    let page = render(&root.join("docs/ledger"))?;
    let current = fs::read_to_string(root.join(OUTPUT)).unwrap_or_default();
    if page != current {
        return Err(format!(
            "{OUTPUT} is out of date with docs/ledger: edit the records in docs/ledger/, \
             never the page, then run `forge ledger generate`"
        ));
    }
    println!("{OUTPUT} is up to date with docs/ledger");
    Ok(())
}

/// `forge ledger list`: one line per record, `<table>/<file>: <first column>`,
/// optionally only rows whose cells contain `filter` (case-insensitive).
pub fn list(root: &Path, filter: Option<&str>) -> Result<(), String> {
    let ledger_dir = root.join("docs/ledger");
    let filter = filter.map(str::to_lowercase);
    let mut seen = BTreeMap::new();
    for (kind_name, table) in tables(&ledger_dir)? {
        let kind = kind(&kind_name).ok_or_else(|| format!("unknown table kind `{kind_name}`"))?;
        for record in load_table(kind, &ledger_dir.join(&table))? {
            let matches = filter
                .as_ref()
                .is_none_or(|f| record.cells.iter().any(|c| c.to_lowercase().contains(f)));
            if matches {
                let file = record
                    .file
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                println!("{table}/{file}: {}", record.cells[0]);
                *seen.entry(table.clone()).or_insert(0) += 1;
            }
        }
    }
    if seen.is_empty() {
        println!("no records match");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn checked_in_ledger_matches_records() {
        let root = workspace_root();
        let rendered = render(&root.join("docs/ledger")).expect("ledger records render");
        let current = fs::read_to_string(root.join(OUTPUT)).expect("ledger page exists");
        assert!(
            rendered == current,
            "{OUTPUT} is out of date with docs/ledger: edit the records, then run \
             `cargo run -p forge-cli -- ledger generate`"
        );
    }

    #[test]
    fn renders_rows_in_order_with_empty_cells() {
        let dir = std::env::temp_dir().join(format!("forge-ledger-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("perf")).unwrap();
        fs::write(
            dir.join("page.md"),
            "# Page\n\n<!-- ledger: metric perf -->\n\nEnd\n",
        )
        .unwrap();
        fs::write(
            dir.join("perf/b.toml"),
            "order = 20\nmetric = \"Second\"\ntarget = \"1 s\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("perf/a.toml"),
            "order = 10\nmetric = \"First\"\ntarget = \"2 s\"\nhow_measured = \"stopwatch\"\nfirst_needed_by = \"Chat\"\nlast_recorded = \"never\"\n",
        )
        .unwrap();
        let page = render(&dir).unwrap();
        assert_eq!(
            page,
            "# Page\n\n\
             | Metric | Proposed target | How measured | First needed by | Last recorded |\n\
             |---|---|---|---|---|\n\
             | First | 2 s | stopwatch | Chat | never |\n\
             | Second | 1 s | | | |\n\nEnd\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_unknown_fields_and_pipes() {
        let kind = kind("metric").unwrap();
        let path = Path::new("x.toml");
        let err =
            parse_record(kind, path, "order = 1\nmetric = \"a\"\ncolour = \"b\"\n").unwrap_err();
        assert!(err.contains("unknown field `colour`"), "{err}");
        let err = parse_record(kind, path, "order = 1\nmetric = \"a | b\"\n").unwrap_err();
        assert!(err.contains("`|`"), "{err}");
        let err = parse_record(kind, path, "metric = \"a\"\n").unwrap_err();
        assert!(err.contains("missing `order`"), "{err}");
    }

    #[test]
    fn parses_markers() {
        assert_eq!(
            marker("<!-- ledger: capability foundation -->"),
            Some(("capability", "foundation"))
        );
        assert_eq!(marker("<!-- ledger: capability -->"), None);
        assert_eq!(marker("| Capability |"), None);
    }
}
