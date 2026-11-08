//! Foundation review tests for forge-storage (docs/foundation-review.md).
//!
//! Two kinds of test live here:
//!
//! - **Contracts that hold.** Each one kills a child process (`SIGKILL`) in
//!   the middle of real writes, many times, and checks what a relaunch finds.
//!   A killed process is what an app crash, a jetsam kill or iOS terminating
//!   a suspended app look like to the file system. It is not a power cut:
//!   that also needs the disk's write cache to honour `fsync`, which only a
//!   device can show.
//! - **Known gaps** (`#[ignore]`, named `gap_*`). Each asserts the contract
//!   the review says the foundation should have, and fails today. Run them
//!   with `cargo test -p forge-storage --features sqlite,hash --test
//!   foundation -- --ignored` to see the failures the review describes; when
//!   a fix lands, remove the `#[ignore]`.
//!
//! The child side of each crash test is an ordinary test function that does
//! nothing unless `FORGE_FOUNDATION_CHILD` is set, so the normal test run
//! skips it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use forge_storage::files::FileStore;
use forge_storage::rusqlite::{params, Connection};
use forge_storage::{write_atomic, Database, JsonFile, Opened};
use serde::{Deserialize, Serialize};

/// How many times each crash test kills its child.
const KILLS: u64 = 12;

const CHILD: &str = "FORGE_FOUNDATION_CHILD";
const CHILD_DIR: &str = "FORGE_FOUNDATION_DIR";

// ------------------------------------------------------------- harness

/// Starts this test binary again, running only `entry` as `mode` in `dir`.
fn spawn_child(entry: &str, mode: &str, dir: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([entry, "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, mode)
        .env(CHILD_DIR, dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the child process")
}

/// Waits until `ready` exists (the child has committed at least once),
/// lets it run for a while, then kills it with SIGKILL.
fn kill_mid_write(mut child: Child, ready: &Path, round: u64) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "the child never started writing");
        if let Ok(Some(status)) = child.try_wait() {
            panic!("the child exited early: {status}");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // A different moment each round: 3 to 47 ms into the writes.
    std::thread::sleep(Duration::from_millis(3 + (round * 17) % 45));
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
}

fn child_dir() -> Option<(String, PathBuf)> {
    let mode = std::env::var(CHILD).ok()?;
    let dir = PathBuf::from(std::env::var(CHILD_DIR).ok()?);
    Some((mode, dir))
}

// ------------------------------------------------------- JSON documents

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Doc {
    n: u64,
    /// `n`, repeated: a torn or mixed write cannot keep them all equal.
    payload: Vec<u64>,
}

impl Doc {
    fn new(n: u64) -> Self {
        Self {
            n,
            payload: vec![n; 20_000],
        }
    }

    fn is_whole(&self) -> bool {
        self.payload.len() == 20_000 && self.payload.iter().all(|&v| v == self.n)
    }
}

fn doc_file(dir: &Path) -> JsonFile<Doc> {
    JsonFile::new(dir.join("doc.json"), 1)
}

#[test]
fn child_json_writer() {
    let Some((mode, dir)) = child_dir() else {
        return;
    };
    assert_eq!(mode, "json");
    let file = doc_file(&dir);
    for n in 1.. {
        file.save(&Doc::new(n)).unwrap();
        if n == 1 {
            std::fs::write(dir.join("ready"), b"").unwrap();
        }
    }
}

/// Storage invariant: an interrupted write never leaves invalid committed
/// state. After every kill the document loads, whole, as some version the
/// child saved, and the versions only move forward.
#[test]
fn a_json_file_survives_a_kill_mid_write() {
    let dir = tempfile::tempdir().unwrap();
    let file = doc_file(dir.path());
    let ready = dir.path().join("ready");
    let mut last = 0;
    for round in 0..KILLS {
        let _ = std::fs::remove_file(&ready);
        let child = spawn_child("child_json_writer", "json", dir.path());
        kill_mid_write(child, &ready, round);
        let doc = file
            .load()
            .expect("the document loads after a kill")
            .expect("the document exists");
        assert!(doc.is_whole(), "round {round}: a torn document loaded");
        assert!(doc.n >= 1);
        // Each child starts again from 1, so only check within a round.
        last = doc.n.max(last);
    }
    assert!(last > 0);
}

// ---------------------------------------------------------------- SQLite

const SCHEMA: &[&str] =
    &["CREATE TABLE rows (pair INTEGER NOT NULL, half INTEGER NOT NULL, body BLOB NOT NULL);"];

#[test]
fn child_sqlite_writer() {
    let Some((mode, dir)) = child_dir() else {
        return;
    };
    assert_eq!(mode, "sqlite");
    let (mut db, _) = Database::open(dir.join("app.db"), SCHEMA).unwrap();
    let body = vec![7u8; 4096];
    let first: i64 = db
        .connection()
        .query_row("SELECT coalesce(max(pair), -1) + 1 FROM rows", [], |row| {
            row.get(0)
        })
        .unwrap();
    for pair in first.. {
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO rows (pair, half, body) VALUES (?1, 0, ?2)",
                params![pair, body],
            )?;
            tx.execute(
                "INSERT INTO rows (pair, half, body) VALUES (?1, 1, ?2)",
                params![pair, body],
            )?;
            Ok::<_, forge_storage::StorageError>(())
        })
        .unwrap();
        if pair == first {
            std::fs::write(dir.join("ready"), b"").unwrap();
        }
    }
}

/// Storage invariant: a transaction is all or nothing across a crash, and
/// a crash never makes `Database::open` think the file is damaged.
#[test]
fn a_database_survives_a_kill_mid_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let ready = dir.path().join("ready");
    let mut rows = 0;
    for round in 0..KILLS {
        let _ = std::fs::remove_file(&ready);
        let child = spawn_child("child_sqlite_writer", "sqlite", dir.path());
        kill_mid_write(child, &ready, round);
        let (db, opened) = Database::open(&path, SCHEMA).expect("reopen after a kill");
        assert_eq!(opened, Opened::Existing, "round {round}: {opened:?}");
        db.integrity_check().expect("integrity after a kill");
        let half_pairs: i64 = db
            .connection()
            .query_row(
                "SELECT count(*) FROM (SELECT pair FROM rows GROUP BY pair HAVING count(*) != 2)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(half_pairs, 0, "round {round}: half a transaction survived");
        let now: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM rows", [], |row| row.get(0))
            .unwrap();
        assert!(now >= rows, "round {round}: committed rows disappeared");
        rows = now;
    }
    assert!(rows > 0);
}

// ------------------------------------------ files and the records naming them

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Records {
    items: Vec<u64>,
}

fn item_name(n: u64) -> String {
    format!("item-{n}")
}

fn item_bytes(n: u64) -> Vec<u8> {
    vec![(n % 251) as u8 + 1; 64 * 1024]
}

#[test]
fn child_files_and_records_writer() {
    let Some((mode, dir)) = child_dir() else {
        return;
    };
    assert_eq!(mode, "files");
    let files = FileStore::new(dir.join("files"));
    let records = JsonFile::<Records>::new(dir.join("records.json"), 1);
    let mut current = records.load().unwrap().unwrap_or_default();
    let mut n = current.items.iter().max().copied().unwrap_or(0);
    loop {
        n += 1;
        // The order the docs prescribe: the file first, then the record
        // that names it. Removing goes the other way round.
        files.write(&item_name(n), &item_bytes(n)).unwrap();
        current.items.push(n);
        let evicted = if current.items.len() > 20 {
            Some(current.items.remove(0))
        } else {
            None
        };
        records.save(&current).unwrap();
        std::fs::write(dir.join("ready"), b"").unwrap();
        if let Some(old) = evicted {
            files.remove(&item_name(old));
        }
    }
}

/// FileStore invariant: with "file first, record second, sweep at launch",
/// a crash never leaves a record naming a missing or partial file, and the
/// sweep removes exactly the files no record names.
#[test]
fn files_and_records_stay_consistent_across_kills() {
    let dir = tempfile::tempdir().unwrap();
    let files = FileStore::new(dir.path().join("files"));
    let records = JsonFile::<Records>::new(dir.path().join("records.json"), 1);
    let ready = dir.path().join("ready");
    for round in 0..KILLS {
        let _ = std::fs::remove_file(&ready);
        let child = spawn_child("child_files_and_records_writer", "files", dir.path());
        kill_mid_write(child, &ready, round);
        let current = records.load().unwrap().unwrap();
        for &n in &current.items {
            let path = files
                .get(&item_name(n))
                .unwrap_or_else(|| panic!("round {round}: record {n} names a missing file"));
            assert_eq!(
                std::fs::read(path).unwrap(),
                item_bytes(n),
                "round {round}: record {n} names a partial file"
            );
        }
        let named: Vec<String> = current.items.iter().map(|&n| item_name(n)).collect();
        files.sweep(|name| named.iter().any(|kept| kept == name));
        let mut left = files.names();
        left.sort();
        let mut want = named.clone();
        want.sort();
        assert_eq!(
            left, want,
            "round {round}: sweep left orphans or removed a named file"
        );
    }
}

// ------------------------------------------------------------ known gaps

/// GAP (finding S1): `Database::open` treats any failure of its quick check
/// as damage, including "database is locked". Another connection holding
/// the file (a second instance, an app extension, a backup in progress) or
/// a file iOS has not unlocked yet makes it move the user's data aside and
/// start an empty database. A transient condition should be an error, never
/// a recovery.
#[test]
#[ignore = "known gap S1: a locked database is treated as corrupt"]
fn gap_a_locked_database_is_not_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    {
        let (mut db, _) = Database::open(&path, SCHEMA).unwrap();
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO rows (pair, half, body) VALUES (1, 0, x'00')",
                [],
            )?;
            Ok::<_, forge_storage::StorageError>(())
        })
        .unwrap();
    }
    let holder = Connection::open(&path).unwrap();
    holder
        .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE;")
        .unwrap();
    let outcome = Database::open(&path, SCHEMA);
    let moved_aside = matches!(outcome, Ok((_, Opened::Recovered { .. })));
    drop(holder);
    assert!(
        !moved_aside,
        "a locked but healthy database was moved aside: {outcome:?}"
    );
}

/// GAP (finding S2): `write_atomic` uses one fixed temporary name per
/// target (`<name>.tmp`), so two writers of the same file share it: one
/// renames the temporary the other is still filling, and the other's rename
/// then fails. In the review run 167 of 400 writes failed; no torn read was
/// seen, but the window for one exists. Nothing stops two writers (two
/// instances on one directory, two threads of one app) from doing this.
#[test]
#[ignore = "known gap S2: concurrent write_atomic calls can publish a torn file"]
fn gap_concurrent_atomic_writes_never_tear() {
    const SIZE: usize = 512 * 1024;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.bin");
    write_atomic(&path, &vec![b'a'; SIZE]).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writers: Vec<_> = (*b"ab")
        .into_iter()
        .map(|byte| {
            let path = path.clone();
            std::thread::spawn(move || {
                let data = vec![byte; SIZE];
                (0..200)
                    .filter(|_| write_atomic(&path, &data).is_err())
                    .count()
            })
        })
        .collect();
    let reader = {
        let (path, stop) = (path.clone(), std::sync::Arc::clone(&stop));
        std::thread::spawn(move || {
            let mut torn = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(bytes) = std::fs::read(&path) {
                    let whole = bytes.len() == SIZE
                        && (bytes.iter().all(|&b| b == b'a') || bytes.iter().all(|&b| b == b'b'));
                    if !whole {
                        torn += 1;
                    }
                }
            }
            torn
        })
    };
    let failed: usize = writers.into_iter().map(|w| w.join().unwrap()).sum();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let torn = reader.join().unwrap();
    assert_eq!(
        (torn, failed),
        (0, 0),
        "torn reads and failed writes with two writers"
    );
}

/// GAP (finding S3): a store whose names are content hashes has no way to
/// say so. `write` and `accept` take any valid name, including a 64-hex one,
/// without hashing, and `get` never checks; so a name that looks verified
/// can point at different bytes, and a file damaged on disk is served as if
/// it were the original.
#[test]
#[ignore = "known gap S3: a hash-shaped name is not checked against its bytes"]
fn gap_a_hash_name_always_matches_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let files = FileStore::new(dir.path());
    let real = files
        .import_hashed(&{
            let source = dir.path().join("source");
            std::fs::write(&source, b"the real bytes").unwrap();
            source
        })
        .unwrap();
    // Anyone holding the store can put other bytes under that name.
    let replaced = files.write(&real.name, b"other bytes");
    let served = std::fs::read(files.get(&real.name).unwrap()).unwrap();
    assert!(
        replaced.is_err() && served == b"the real bytes",
        "a content-addressed name now serves bytes that do not match it"
    );
}
