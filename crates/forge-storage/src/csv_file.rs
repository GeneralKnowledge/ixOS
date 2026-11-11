//! CSV import and export (feature `csv`): the format people move lists
//! between apps and spreadsheets with. Rows are any serde type, so an app
//! writes one struct and gets both directions.
//!
//! Export is atomic (the file is complete or absent) and starts with a UTF-8
//! byte-order mark, so Excel and Numbers read accents correctly. Import is
//! forgiving the way a person's spreadsheet needs: it skips a byte-order
//! mark, matches headers ignoring case, spaces and underscores, ignores
//! extra columns, and keeps every good row while reporting each bad one by
//! line number, so one typo doesn't lose the whole file.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{write_atomic, StorageError};

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Most rows an import reads; a bigger file is refused rather than filling
/// memory.
pub const MAX_IMPORT_ROWS: usize = 100_000;

/// One row that could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvProblem {
    /// The line in the file (the header is line 1).
    pub line: u64,
    pub reason: String,
}

/// What an import found: the rows it could read, in order, and the ones it
/// could not.
#[derive(Clone, Debug, PartialEq)]
pub struct CsvImport<T> {
    pub rows: Vec<T>,
    pub problems: Vec<CsvProblem>,
}

/// Writes `rows` to `path` as CSV with a header row taken from `T`'s field
/// names (serde renames apply). Atomic: a reader sees the old file or the
/// whole new one.
pub fn export_csv<T: Serialize>(path: &Path, rows: &[T]) -> Result<(), StorageError> {
    write_atomic(path, &to_csv(rows)?)
}

/// `rows` as CSV bytes, for sharing without a file.
pub fn to_csv<T: Serialize>(rows: &[T]) -> Result<Vec<u8>, StorageError> {
    let mut writer = csv::Writer::from_writer(BOM.to_vec());
    for row in rows {
        writer
            .serialize(row)
            .map_err(|e| StorageError::write("write a CSV row", e))?;
    }
    writer
        .into_inner()
        .map_err(|e| StorageError::write("finish the CSV", e))
}

/// Reads the CSV file at `path` into rows of `T`.
pub fn import_csv<T: DeserializeOwned>(path: &Path) -> Result<CsvImport<T>, StorageError> {
    let bytes = std::fs::read(path).map_err(|e| StorageError::read("read the CSV", e))?;
    from_csv(&bytes)
}

/// Reads CSV bytes into rows of `T`. Fails only when the file has no header
/// or is too big; a row that doesn't fit `T` becomes a [`CsvProblem`].
pub fn from_csv<T: DeserializeOwned>(bytes: &[u8]) -> Result<CsvImport<T>, StorageError> {
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(bytes);
    // Each column goes to the field whose name it matches loosely.
    let fields = struct_fields::<T>();
    let headers = reader
        .headers()
        .map_err(|e| StorageError::corrupt(format!("the CSV has no header row: {e}")))?
        .iter()
        .map(|header| {
            let key = normalise(header);
            fields
                .iter()
                .find(|field| normalise(field) == key)
                .map_or_else(|| header.trim().to_string(), |field| field.to_string())
        })
        .collect::<csv::StringRecord>();
    if headers.is_empty() {
        return Err(StorageError::corrupt("the CSV has no header row"));
    }
    reader.set_headers(headers.clone());
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (index, record) in reader.records().enumerate() {
        if index >= MAX_IMPORT_ROWS {
            return Err(StorageError::corrupt(format!(
                "the CSV has more than {MAX_IMPORT_ROWS} rows"
            )));
        }
        let record = match record {
            Ok(record) => record,
            Err(e) => {
                let line = e.position().map_or(index as u64 + 2, |p| line_at(bytes, p));
                problems.push(CsvProblem {
                    line,
                    reason: e.to_string(),
                });
                continue;
            }
        };
        let line = record
            .position()
            .map_or(index as u64 + 2, |p| line_at(bytes, p));
        // A blank line in a spreadsheet export is not a problem.
        if record.iter().all(str::is_empty) {
            continue;
        }
        match record.deserialize::<T>(Some(&headers)) {
            Ok(row) => rows.push(row),
            Err(e) => problems.push(CsvProblem {
                line,
                reason: describe(&e),
            }),
        }
    }
    Ok(CsvImport { rows, problems })
}

/// The line a record starts on, counted from the bytes (the reader's own
/// count leaves out blank lines it skipped).
fn line_at(bytes: &[u8], position: &csv::Position) -> u64 {
    let mut end = (position.byte() as usize).min(bytes.len());
    // A record's position can be the blank lines before it.
    while end < bytes.len() && (bytes[end] == b'\n' || bytes[end] == b'\r') {
        end += 1;
    }
    1 + bytes[..end].iter().filter(|b| **b == b'\n').count() as u64
}

/// "Due Date", "due_date" and "DUE DATE" all match the field `due_date`.
fn normalise(header: &str) -> String {
    header
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The field names of a struct `T` derives `Deserialize` for (empty for
/// anything else), found by asking `T` to deserialize from a deserializer
/// that only records what it is asked for.
fn struct_fields<T: DeserializeOwned>() -> &'static [&'static str] {
    use serde::de::{self, Visitor};
    use std::cell::Cell;

    #[derive(Debug)]
    struct Stop;
    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("stop")
        }
    }
    impl std::error::Error for Stop {}
    impl de::Error for Stop {
        fn custom<M: std::fmt::Display>(_: M) -> Self {
            Stop
        }
    }

    struct Fields<'a>(&'a Cell<&'static [&'static str]>);
    impl<'de> de::Deserializer<'de> for Fields<'_> {
        type Error = Stop;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
            Err(Stop)
        }
        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Stop> {
            self.0.set(fields);
            Err(Stop)
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }

    let fields = Cell::new(&[][..]);
    let _ = T::deserialize(Fields(&fields));
    fields.get()
}

fn describe(e: &csv::Error) -> String {
    match e.kind() {
        csv::ErrorKind::Deserialize { err, .. } => match err.field() {
            Some(field) => format!("column {}: {}", field + 1, err.kind()),
            None => err.kind().to_string(),
        },
        _ => e.to_string(),
    }
}
