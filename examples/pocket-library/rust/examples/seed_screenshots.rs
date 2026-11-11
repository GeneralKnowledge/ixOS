//! Writes a realistic library for screenshots: books in every status, a few
//! covers and two weeks of reading history, dated relative to now. macOS CI
//! puts it in the simulator app's container (scripts/pocket-library-screenshots.sh).
//!
//! cargo run --manifest-path rust/Cargo.toml --example seed_screenshots -- <dir>

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::write::ZlibEncoder;
use flate2::{Compression, Crc};
use forge_ffi::domain::{validate, LibraryData, SavedPosition};
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
    // The seed replaces the whole library, files included.
    store.clear_files();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let mut data = LibraryData::default();
    data.settings.default_sort = LibrarySort::RecentlyAdded;

    for seed in BOOKS {
        let cover_file = seed.cover.map(|(background, band)| {
            store
                .stage_cover(&slug(seed.title), "png", &cover_png(background, band))
                .unwrap()
                .commit()
                .name
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
        // One book has its EPUB, opened at its first chapter, for the
        // reader screenshot (`-openReader <id>`).
        if seed.title == "Night Trains" {
            book.epub_file = Some(
                store
                    .stage_book_file("night-trains", &night_trains_epub())
                    .unwrap()
                    .commit()
                    .name,
            );
            book.reading_position = Some(SavedPosition {
                chapter: 0,
                fraction: 0.0,
                page: book.current_page,
            });
            println!("[seed] readable book id {id}");
        }
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

/// A short EPUB with two chapters of original text (no real book is
/// bundled), using the formatting the reader shows: headings, italics,
/// a scene break and a quotation.
fn night_trains_epub() -> Vec<u8> {
    let chapters = [
        (
            "One: The Last Sleeper",
            r#"<h1>One: The Last Sleeper</h1>
<p>The train left the city a little after ten, when the platforms had emptied and the station clock had begun to sound louder than the announcements. Lena found her compartment near the front, a narrow room with two bunks, a folding table and a window that would not quite close.</p>
<p>She had bought the ticket on an impulse, the way people buy a coat they are not sure they need. <em>Somewhere east</em>, she had told the clerk, and he had smiled as if he heard that every night.</p>
<p>The conductor came by with a clipboard and a tired kindness. He asked whether she wanted tea in the morning, and whether she would be getting off before the border or after it.</p>
<blockquote><p>“Most people know,” he said. “The ones who don’t usually sleep best.”</p></blockquote>
<p>She did not know. She lay on the lower bunk and watched the lights of the suburbs thin out into villages, and the villages into dark fields where only a single farmhouse window kept watch.</p>
<p class="center">* * *</p>
<p>Somewhere after midnight the train stopped in a station with no name she could read. A woman with a cello case climbed aboard, looked at the number on Lena’s door, and laughed quietly when she realised it was hers too.</p>
<p>“I hope you don’t mind music,” she said. “I never play it. I only carry it.”</p>"#,
        ),
        (
            "Two: Border Light",
            r#"<h1>Two: Border Light</h1>
<p>Morning came up grey and then gold over a river that seemed too wide for the country it ran through. The cellist was already awake, sitting by the window with her hands around a paper cup.</p>
<p>They talked about nothing for an hour: the best bread they had ever eaten, cities that looked better in the rain, a dog one of them had loved as a child. It was the kind of conversation that only happens between strangers who know they will part at the next long platform.</p>
<p>At the border the train waited while men in blue walked its length with torches they no longer needed. Lena realised she had decided, somewhere in the night, that she would not be getting off before it.</p>"#,
        ),
    ];
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, body: &str| {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    add("mimetype", "application/epub+zip");
    add(
        "META-INF/container.xml",
        r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    );
    add(
        "OEBPS/content.opf",
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Night Trains</dc:title><dc:creator>Ines Kovač</dc:creator></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="one.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="two.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#,
    );
    add(
        "OEBPS/nav.xhtml",
        &format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml">{}</a></li><li><a href="two.xhtml">{}</a></li></ol></nav></body></html>"#,
            chapters[0].0, chapters[1].0
        ),
    );
    for (file, (_, body)) in ["one.xhtml", "two.xhtml"].iter().zip(chapters) {
        add(
            &format!("OEBPS/{file}"),
            &format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{body}</body></html>"#),
        );
    }
    zip.finish().unwrap().into_inner()
}
