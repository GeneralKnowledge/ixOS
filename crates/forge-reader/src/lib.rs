//! ForgeReader: the reusable reading experience's Rust half
//! (docs/forge-reader.md). It is not tied to any format: an EPUB chapter
//! and a feed article both become a [`ReaderSection`] of styled
//! [`ReaderBlock`]s, each with an id that stays the same when the text
//! around it changes, and a reading position is a [`ReaderAnchor`] on one
//! of those ids.
//!
//! - [`blocks_from_html`] converts HTML or XHTML; the app says where its
//!   pictures and links go ([`HtmlResources`], or [`WebResources`] for web
//!   content).
//! - [`ReaderAnchor::at`] saves a place; [`ReaderAnchor::resolve`] finds it
//!   again, falling back to an edited block's opening words, its
//!   neighbours, then the section's start.
//! - [`section_progress`] and [`location_at`] map places to fractions by
//!   words, for progress bars and page numbers.
//!
//! An app exposes the types to Swift with [`reader_types!`], and gets the
//! SwiftUI reader view (`ForgeReaderSectionView` and friends) in its
//! bindings module with them.

mod anchor;
mod document;
mod html;

pub use anchor::{location_at, section_progress, AnchorMatch, ReaderAnchor, ReaderLocation};
pub use document::{assign_ids, ReaderBlock, ReaderBlockKind, ReaderSection, ReaderSpan};
pub use html::{attribute, blocks_from_html, resolve_entity, HtmlResources, WebResources};

/// Declares the reader's types for UniFFI in the calling app crate, after
/// its `uniffi::setup_scaffolding!()`: `ReaderSpan`, `ReaderBlockKind`,
/// `ReaderBlock`, `ReaderSection`, `ReaderAnchor` and `ReaderLocation`
/// become records of the app's own bindings (and are imported into the
/// module that calls it, so the app's records can hold them). `forge
/// bindings generate` then adds ForgeReader's SwiftUI views to the app's
/// Swift module.
///
/// The app crate needs `uniffi` as a direct dependency.
#[macro_export]
macro_rules! reader_types {
    () => {
        pub use $crate::{
            ReaderAnchor, ReaderBlock, ReaderBlockKind, ReaderLocation, ReaderSection, ReaderSpan,
        };

        #[::uniffi::remote(Record)]
        pub struct ReaderSpan {
            pub text: String,
            pub bold: bool,
            pub italic: bool,
            pub monospace: bool,
            pub link: Option<String>,
        }

        #[::uniffi::remote(Enum)]
        pub enum ReaderBlockKind {
            Heading { level: u8 },
            Paragraph,
            Quote,
            ListItem { marker: String },
            Preformatted,
            Separator,
            Image { source: String, alt: String },
        }

        #[::uniffi::remote(Record)]
        pub struct ReaderBlock {
            pub id: String,
            pub kind: ReaderBlockKind,
            pub spans: Vec<ReaderSpan>,
        }

        #[::uniffi::remote(Record)]
        pub struct ReaderSection {
            pub id: String,
            pub title: String,
            pub blocks: Vec<ReaderBlock>,
        }

        #[::uniffi::remote(Record)]
        pub struct ReaderAnchor {
            pub section_id: String,
            pub block_id: String,
            pub offset: f64,
            pub opening: String,
            pub before: Option<String>,
            pub after: Option<String>,
        }

        #[::uniffi::remote(Record)]
        pub struct ReaderLocation {
            pub block: u32,
            pub offset: f64,
        }
    };
}
