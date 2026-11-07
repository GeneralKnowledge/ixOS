//! Writes a realistic library for screenshots: books in every status, a few
//! covers and two weeks of reading history, dated relative to now. macOS CI
//! puts it in the simulator app's container (scripts/pocket-library-screenshots.sh).
//!
//! cargo run --manifest-path rust/Cargo.toml --example seed_screenshots -- <dir>

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::write::ZlibEncoder;
use flate2::{Compression, Crc};
use forge_ffi::domain::{validate, LibraryData};
use forge_ffi::store::Store;
use forge_ffi::{BookInput, BookSource, LibrarySort, ReadingStatus};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

struct Seed {
    title: &'static str,
    author: &'static str,
    pages: u32,
    rating: Option<u8>,
    /// Cover colours (background, band), or no cover.
    cover: Option<([u8; 3], [u8; 3])>,
    description: &'static str,
    /// (days ago, page reached); the last page finishes the book.
    reading: &'static [(i64, u32)],
    added_days_ago: i64,
}

const BOOKS: &[Seed] = &[
    Seed {
        title: "The Salt Road",
        author: "Mara Ellison",
        pages: 312,
        rating: Some(5),
        cover: Some(([38, 70, 83], [233, 196, 106])),
        description: "Three generations of a family follow the old trade route across the desert.",
        reading: &[(20, 60), (18, 140), (15, 230), (13, 312)],
        added_days_ago: 30,
    },
    Seed {
        title: "A Field Guide to Clouds",
        author: "Tomas Reyes",
        pages: 224,
        rating: Some(4),
        cover: Some(([69, 123, 157], [241, 250, 238])),
        description: "How to read the sky, from cirrus to cumulonimbus.",
        reading: &[(11, 50), (9, 120), (6, 224)],
        added_days_ago: 14,
    },
    Seed {
        title: "Night Trains",
        author: "Ines Kovač",
        pages: 388,
        rating: None,
        cover: Some(([29, 53, 87], [230, 57, 70])),
        description: "Stories set on sleeper trains across Europe.",
        reading: &[(5, 40), (3, 96), (1, 151), (0, 187)],
        added_days_ago: 8,
    },
    Seed {
        title: "The Quiet Workshop",
        author: "Daniel Okafor",
        pages: 256,
        rating: None,
        cover: None,
        description: "Notes on making things slowly and well.",
        reading: &[(4, 22), (2, 64)],
        added_days_ago: 6,
    },
    Seed {
        title: "Harbour Lights",
        author: "Annika Berg",
        pages: 410,
        rating: None,
        cover: Some(([42, 157, 143], [244, 162, 97])),
        description: "",
        reading: &[],
        added_days_ago: 3,
    },
    Seed {
        title: "An Atlas of Small Islands",
        author: "Priya Natarajan",
        pages: 198,
        rating: None,
        cover: None,
        description: "Fifty islands most maps leave out.",
        reading: &[],
        added_days_ago: 1,
    },
    Seed {
        title: "Winter Orchard",
        author: "Mara Ellison",
        pages: 290,
        rating: Some(4),
        cover: None,
        description: "",
        reading: &[(40, 150), (37, 290)],
        added_days_ago: 45,
    },
];

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: seed_screenshots <dir>");
    let store = Store::new(&dir);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let mut data = LibraryData::default();
    data.settings.default_sort = LibrarySort::RecentlyAdded;

    for seed in BOOKS {
        let cover_file = seed.cover.map(|(background, band)| {
            store
                .save_cover(&slug(seed.title), "png", &cover_png(background, band))
                .unwrap()
        });
        let input = validate(BookInput {
            title: seed.title.into(),
            author: seed.author.into(),
            description: seed.description.into(),
            status: ReadingStatus::WantToRead,
            current_page: 0,
            total_pages: Some(seed.pages),
            rating: None,
            notes: String::new(),
            cover_url: None,
        })
        .unwrap();
        let added = now - seed.added_days_ago * DAY - 2 * HOUR;
        let id = data
            .add(input, BookSource::Manual, cover_file, added)
            .unwrap();
        for &(days_ago, page) in seed.reading {
            let status = if page >= seed.pages {
                ReadingStatus::Finished
            } else {
                ReadingStatus::Reading
            };
            data.set_reading_state(id, status, page, now - days_ago * DAY - HOUR)
                .unwrap();
        }
        let book = data.books.iter_mut().find(|b| b.id == id).unwrap();
        book.rating = seed.rating;
    }
    store.save(&data).unwrap();
    println!("[seed] {} books in {dir}", data.books.len());
}

fn slug(title: &str) -> String {
    title.to_lowercase().replace(' ', "-")
}

/// A 300x450 PNG: a solid cover with a band where a title would be.
fn cover_png(background: [u8; 3], band: [u8; 3]) -> Vec<u8> {
    let (width, height) = (300u32, 450u32);
    let mut raw = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height {
        raw.push(0); // no filter
        let colour = if (110..170).contains(&y) || (380..392).contains(&y) {
            band
        } else {
            background
        };
        for _ in 0..width {
            raw.extend_from_slice(&colour);
        }
    }
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
    zlib.write_all(&raw).unwrap();
    let idat = zlib.finish().unwrap();

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(b"IHDR", ihdr), (b"IDAT", idat), (b"IEND", Vec::new())] {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut crc = Crc::new();
        crc.update(kind);
        crc.update(&body);
        png.extend_from_slice(kind);
        png.extend_from_slice(&body);
        png.extend_from_slice(&crc.sum().to_be_bytes());
    }
    png
}
