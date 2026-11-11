//! FieldKit's SQLite file: trips, their track points and their notes. Only
//! the command thread touches it, and every change is one transaction.

use std::path::Path;

use forge_storage::rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use forge_storage::{Database, Opened};

use crate::model::{FieldKitError, Note, NoteKind, Position, TripSummary};
use crate::track::{Totals, TrackPoint};

/// Schema steps: step `i` brings the file from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE trips (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        started_ms INTEGER NOT NULL,
        ended_ms INTEGER,
        distance_m REAL NOT NULL DEFAULT 0,
        climb_m REAL NOT NULL DEFAULT 0,
        climb_base_m REAL,
        point_count INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE points (
        trip_id INTEGER NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
        seq INTEGER NOT NULL,
        latitude REAL NOT NULL,
        longitude REAL NOT NULL,
        altitude_m REAL,
        accuracy_m REAL NOT NULL,
        timestamp_ms INTEGER NOT NULL,
        PRIMARY KEY (trip_id, seq)
    ) WITHOUT ROWID;
    CREATE TABLE notes (
        id INTEGER PRIMARY KEY,
        trip_id INTEGER NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
        created_ms INTEGER NOT NULL,
        latitude REAL,
        longitude REAL,
        kind TEXT NOT NULL CHECK (kind IN ('text', 'photo', 'voice')),
        text TEXT NOT NULL,
        file TEXT,
        duration_ms INTEGER
    );
    CREATE INDEX notes_by_trip ON notes(trip_id, created_ms);
    -- At most one trip is recording (has no end) at a time.
    CREATE UNIQUE INDEX one_recording ON trips((ended_ms IS NULL)) WHERE ended_ms IS NULL;
"];

/// A note as stored: its file is a name in the media store, not a path.
pub enum NewNote {
    Text { body: String },
    Photo { file: String, caption: String },
    Voice { file: String, duration_ms: i64 },
}

pub struct Store {
    db: Database,
}

impl Store {
    /// Opens `fieldkit.sqlite` in `directory`. A damaged file is moved aside
    /// (kept) and a fresh one made; the core logs it.
    pub fn open(directory: &Path) -> Result<(Self, Opened), FieldKitError> {
        let (db, opened) = Database::open(directory.join("fieldkit.sqlite"), MIGRATIONS)?;
        db.connection().execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok((Self { db }, opened))
    }

    /// A second, read-only connection to the same file, for queries that
    /// run off the command thread (WAL keeps them apart from the writer).
    pub fn reader(&self) -> Result<Connection, FieldKitError> {
        let path = self.db.path().expect("opened from a file");
        let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(c)
    }

    pub fn trips(&self) -> Result<Vec<TripSummary>, FieldKitError> {
        trips(self.db.connection())
    }

    pub fn exists(&self, trip_id: i64) -> Result<bool, FieldKitError> {
        Ok(self
            .db
            .connection()
            .query_row("SELECT 1 FROM trips WHERE id = ?1", [trip_id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// The trip being recorded, with its totals and last kept point.
    pub fn recording(&self) -> Result<Option<(i64, Totals, Option<TrackPoint>)>, FieldKitError> {
        let c = self.db.connection();
        let trip = c
            .query_row(
                "SELECT id, distance_m, climb_m, climb_base_m, point_count
                 FROM trips WHERE ended_ms IS NULL",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        Totals {
                            distance_m: r.get(1)?,
                            climb_m: r.get(2)?,
                            climb_base_m: r.get(3)?,
                            point_count: r.get(4)?,
                        },
                    ))
                },
            )
            .optional()?;
        let Some((id, totals)) = trip else {
            return Ok(None);
        };
        let last = c
            .query_row(
                "SELECT latitude, longitude, altitude_m, accuracy_m, timestamp_ms
                 FROM points WHERE trip_id = ?1 ORDER BY seq DESC LIMIT 1",
                [id],
                track_point,
            )
            .optional()?;
        Ok(Some((id, totals, last)))
    }

    pub fn start_trip(&mut self, title: &str, started_ms: i64) -> Result<i64, FieldKitError> {
        let c = self.db.connection();
        c.execute(
            "INSERT INTO trips (title, started_ms) VALUES (?1, ?2)",
            params![title, started_ms],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// Appends kept points to a trip and saves its new totals, together.
    pub fn append(
        &mut self,
        trip_id: i64,
        first_seq: u32,
        points: &[TrackPoint],
        totals: &Totals,
    ) -> Result<(), FieldKitError> {
        self.db.transaction(|tx| {
            {
                let mut insert = tx.prepare_cached(
                    "INSERT INTO points (trip_id, seq, latitude, longitude, altitude_m,
                                         accuracy_m, timestamp_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for (i, p) in points.iter().enumerate() {
                    insert.execute(params![
                        trip_id,
                        first_seq + i as u32,
                        p.latitude,
                        p.longitude,
                        p.altitude_m,
                        p.horizontal_accuracy_m,
                        p.timestamp_ms
                    ])?;
                }
            }
            tx.execute(
                "UPDATE trips SET distance_m = ?2, climb_m = ?3, climb_base_m = ?4,
                                  point_count = ?5 WHERE id = ?1",
                params![
                    trip_id,
                    totals.distance_m,
                    totals.climb_m,
                    totals.climb_base_m,
                    totals.point_count
                ],
            )?;
            Ok::<_, FieldKitError>(())
        })
    }

    pub fn end_trip(&mut self, trip_id: i64, ended_ms: i64) -> Result<(), FieldKitError> {
        self.db.connection().execute(
            "UPDATE trips SET ended_ms = ?2 WHERE id = ?1",
            params![trip_id, ended_ms],
        )?;
        Ok(())
    }

    pub fn rename_trip(&mut self, trip_id: i64, title: &str) -> Result<(), FieldKitError> {
        let changed = self.db.connection().execute(
            "UPDATE trips SET title = ?2 WHERE id = ?1",
            params![trip_id, title],
        )?;
        if changed == 0 {
            return Err(FieldKitError::NotFound { id: trip_id });
        }
        Ok(())
    }

    /// Deletes a trip with its points and notes; returns the media files its
    /// notes used, for the caller to remove if nothing else uses them.
    pub fn delete_trip(&mut self, trip_id: i64) -> Result<Vec<String>, FieldKitError> {
        self.db.transaction(|tx| {
            let files = {
                let mut stmt =
                    tx.prepare("SELECT file FROM notes WHERE trip_id = ?1 AND file IS NOT NULL")?;
                let files = stmt
                    .query_map([trip_id], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                files
            };
            if tx.execute("DELETE FROM trips WHERE id = ?1", [trip_id])? == 0 {
                return Err(FieldKitError::NotFound { id: trip_id });
            }
            Ok(files)
        })
    }

    pub fn add_note(
        &mut self,
        trip_id: i64,
        created_ms: i64,
        at: Option<Position>,
        note: &NewNote,
    ) -> Result<i64, FieldKitError> {
        let (kind, text, file, duration): (&str, &str, Option<&str>, Option<i64>) = match note {
            NewNote::Text { body } => ("text", body, None, None),
            NewNote::Photo { file, caption } => ("photo", caption, Some(file), None),
            NewNote::Voice { file, duration_ms } => ("voice", "", Some(file), Some(*duration_ms)),
        };
        let c = self.db.connection();
        c.execute(
            "INSERT INTO notes (trip_id, created_ms, latitude, longitude, kind, text, file,
                                duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                trip_id,
                created_ms,
                at.map(|p| p.latitude),
                at.map(|p| p.longitude),
                kind,
                text,
                file,
                duration
            ],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// Removes a note; returns its media file, if it had one.
    pub fn remove_note(&mut self, note_id: i64) -> Result<Option<String>, FieldKitError> {
        let c = self.db.connection();
        let file = c
            .query_row("SELECT file FROM notes WHERE id = ?1", [note_id], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()?
            .ok_or(FieldKitError::NotFound { id: note_id })?;
        c.execute("DELETE FROM notes WHERE id = ?1", [note_id])?;
        Ok(file)
    }

    /// Whether any note still uses media file `name`.
    pub fn file_in_use(&self, name: &str) -> Result<bool, FieldKitError> {
        Ok(self
            .db
            .connection()
            .query_row(
                "SELECT 1 FROM notes WHERE file = ?1 LIMIT 1",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

/// Every trip, newest first.
pub fn trips(c: &Connection) -> Result<Vec<TripSummary>, FieldKitError> {
    let mut stmt = c.prepare(
        "SELECT t.id, t.title, t.started_ms, t.ended_ms, t.distance_m, t.climb_m,
                t.point_count, (SELECT COUNT(*) FROM notes n WHERE n.trip_id = t.id)
         FROM trips t ORDER BY t.started_ms DESC, t.id DESC",
    )?;
    let trips = stmt
        .query_map([], trip_summary)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(trips)
}

/// A trip's kept track, in order.
pub fn track(c: &Connection, trip_id: i64) -> Result<Vec<TrackPoint>, FieldKitError> {
    let mut stmt = c.prepare(
        "SELECT latitude, longitude, altitude_m, accuracy_m, timestamp_ms
         FROM points WHERE trip_id = ?1 ORDER BY seq",
    )?;
    let points = stmt
        .query_map([trip_id], track_point)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(points)
}

/// A trip's notes, oldest first. `path` turns a stored file name into
/// the path Swift opens.
pub fn notes(
    c: &Connection,
    trip_id: i64,
    path: impl Fn(&str) -> String,
) -> Result<Vec<Note>, FieldKitError> {
    let mut stmt = c.prepare(
        "SELECT id, trip_id, created_ms, latitude, longitude, kind, text, file, duration_ms
         FROM notes WHERE trip_id = ?1 ORDER BY created_ms, id",
    )?;
    let notes = stmt
        .query_map([trip_id], |r| {
            let latitude: Option<f64> = r.get(3)?;
            let longitude: Option<f64> = r.get(4)?;
            let kind: String = r.get(5)?;
            let text: String = r.get(6)?;
            let file: Option<String> = r.get(7)?;
            let file = file.map(|f| path(&f)).unwrap_or_default();
            let kind = match kind.as_str() {
                "photo" => NoteKind::Photo {
                    path: file,
                    caption: text,
                },
                "voice" => NoteKind::Voice {
                    path: file,
                    duration_ms: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
                },
                _ => NoteKind::Text { body: text },
            };
            Ok(Note {
                id: r.get(0)?,
                trip_id: r.get(1)?,
                created_ms: r.get(2)?,
                at: latitude
                    .zip(longitude)
                    .map(|(latitude, longitude)| Position {
                        latitude,
                        longitude,
                    }),
                kind,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}

fn trip_summary(r: &Row) -> forge_storage::rusqlite::Result<TripSummary> {
    let ended_ms: Option<i64> = r.get(3)?;
    Ok(TripSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        started_ms: r.get(2)?,
        ended_ms,
        recording: ended_ms.is_none(),
        distance_m: r.get(4)?,
        climb_m: r.get(5)?,
        point_count: r.get(6)?,
        note_count: r.get(7)?,
    })
}

fn track_point(r: &Row) -> forge_storage::rusqlite::Result<TrackPoint> {
    Ok(TrackPoint {
        latitude: r.get(0)?,
        longitude: r.get(1)?,
        altitude_m: r.get(2)?,
        horizontal_accuracy_m: r.get(3)?,
        timestamp_ms: r.get(4)?,
    })
}
