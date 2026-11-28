//! Reading a profile pack: what BusinessForge's extractor writes for one
//! business. A pack is a folder or a zip of that folder, holding
//! `profile.json`, the images it names (`images/logo.png`), and optionally
//! `hashes.json`, the SHA-256 of each file. The extractor's other files (its
//! review list, the page text it read) are left behind: the phone needs only
//! what the preview shows. A lone `profile.json` is a pack with no images.
//!
//! Every rule a pack must follow is here, so a pack the phone keeps is one
//! the preview can show: the profile is valid for this schema version, every
//! image it names is in the pack and is a PNG or JPEG of a sensible size,
//! and every file matches its hash when the pack lists hashes.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};

use business_profile::Profile;
use sha2::{Digest, Sha256};

/// Largest `profile.json`, in bytes.
pub const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
/// Largest image, in bytes.
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// Widest or tallest image, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 6000;
/// Largest pack file (a zip, or a lone profile), in bytes.
pub const MAX_PACK_BYTES: u64 = 64 * 1024 * 1024;
/// Most entries a zipped pack may hold (the extractor's page text included).
pub const MAX_ZIP_ENTRIES: usize = 500;

const PROFILE: &str = "profile.json";
const HASHES: &str = "hashes.json";

/// Why a pack was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// Not a folder, zip or profile the preview can read.
    NotAPack(String),
    /// The profile breaks the schema's rules, one line per problem.
    Invalid(Vec<String>),
    TooLarge {
        what: String,
        max_bytes: u64,
    },
    BadImage {
        path: String,
        reason: String,
    },
    HashMismatch {
        path: String,
    },
    Io(String),
}

/// A pack that passed every check.
#[derive(Debug, Clone)]
pub struct Pack {
    pub profile: Profile,
    /// `profile.json` as it came, so the copy kept is the one checked.
    pub profile_json: Vec<u8>,
    /// Each image the profile names, by its path in the pack.
    pub images: BTreeMap<String, Vec<u8>>,
}

impl Pack {
    /// Every file to keep, by path in the pack.
    pub fn files(&self) -> BTreeMap<String, &[u8]> {
        let mut files: BTreeMap<String, &[u8]> = self
            .images
            .iter()
            .map(|(k, v)| (k.clone(), v.as_slice()))
            .collect();
        files.insert(PROFILE.to_string(), &self.profile_json);
        files
    }
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads and checks the pack at `path`: a folder, a `.zip`, or a lone
/// profile (any other file).
pub fn read(path: &Path) -> Result<Pack, PackError> {
    let meta = std::fs::metadata(path).map_err(|e| PackError::NotAPack(e.to_string()))?;
    if meta.is_dir() {
        return check(&mut Folder(path.to_path_buf()));
    }
    if meta.len() > MAX_PACK_BYTES {
        return Err(PackError::TooLarge {
            what: "the pack".into(),
            max_bytes: MAX_PACK_BYTES,
        });
    }
    let mut file = File::open(path).map_err(|e| PackError::Io(e.to_string()))?;
    let mut magic = [0u8; 4];
    let n = file
        .read(&mut magic)
        .map_err(|e| PackError::Io(e.to_string()))?;
    file.rewind().map_err(|e| PackError::Io(e.to_string()))?;
    if n == 4 && magic == *b"PK\x03\x04" {
        check(&mut Zipped::open(file)?)
    } else {
        check(&mut Lone(file))
    }
}

/// Where a pack's files come from.
trait Files {
    /// The file at `path` in the pack, or None if there is none. Refuses a
    /// file over `max` bytes.
    fn get(&mut self, path: &str, max: u64, what: &str) -> Result<Option<Vec<u8>>, PackError>;
}

fn read_capped(reader: impl Read, max: u64, what: &str) -> Result<Vec<u8>, PackError> {
    let mut bytes = Vec::new();
    reader
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| PackError::Io(e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(PackError::TooLarge {
            what: what.to_string(),
            max_bytes: max,
        });
    }
    Ok(bytes)
}

struct Folder(PathBuf);

impl Files for Folder {
    fn get(&mut self, path: &str, max: u64, what: &str) -> Result<Option<Vec<u8>>, PackError> {
        let full = self.0.join(path);
        // Only plain files inside the folder: never a link out of it.
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => return Err(PackError::NotAPack(format!("{path} isn't a plain file"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(PackError::Io(e.to_string())),
        }
        let file = File::open(&full).map_err(|e| PackError::Io(e.to_string()))?;
        read_capped(file, max, what).map(Some)
    }
}

/// A profile on its own: `profile.json` is the file, and nothing else exists.
struct Lone(File);

impl Files for Lone {
    fn get(&mut self, path: &str, max: u64, what: &str) -> Result<Option<Vec<u8>>, PackError> {
        if path != PROFILE {
            return Ok(None);
        }
        self.0.rewind().map_err(|e| PackError::Io(e.to_string()))?;
        read_capped(&mut self.0, max, what).map(Some)
    }
}

struct Zipped {
    archive: zip::ZipArchive<File>,
    /// The folder the pack's files are in, when the zip holds one (a zipped
    /// pack folder): `riding-school/`, or empty.
    prefix: String,
}

impl Zipped {
    fn open(file: File) -> Result<Self, PackError> {
        let archive = zip::ZipArchive::new(file)
            .map_err(|e| PackError::NotAPack(format!("not a readable zip: {e}")))?;
        if archive.len() > MAX_ZIP_ENTRIES {
            return Err(PackError::TooLarge {
                what: format!("the zip ({} files)", archive.len()),
                max_bytes: MAX_PACK_BYTES,
            });
        }
        // profile.json at the top, or in one folder at the top.
        let mut prefixes: Vec<String> = archive
            .file_names()
            .filter_map(|name| Some(name.ok()?.strip_suffix(PROFILE)?.to_string()))
            .filter(|prefix| {
                prefix.is_empty()
                    || (prefix.ends_with('/') && !prefix[..prefix.len() - 1].contains('/'))
            })
            .collect();
        prefixes.sort_by_key(|p| p.len());
        match prefixes.as_slice() {
            [] => Err(PackError::NotAPack("the zip has no profile.json".into())),
            [one] => Ok(Self {
                prefix: one.clone(),
                archive,
            }),
            [first, second, ..] if first.is_empty() && !second.is_empty() => Ok(Self {
                prefix: String::new(),
                archive,
            }),
            _ => Err(PackError::NotAPack(
                "the zip holds more than one business; send one pack at a time".into(),
            )),
        }
    }
}

impl Files for Zipped {
    fn get(&mut self, path: &str, max: u64, what: &str) -> Result<Option<Vec<u8>>, PackError> {
        let name = format!("{}{path}", self.prefix);
        let entry = match self.archive.by_name(&name) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(PackError::NotAPack(format!("{path} can't be read: {e}"))),
        };
        if !entry.is_file() {
            return Err(PackError::NotAPack(format!("{path} isn't a plain file")));
        }
        // The size in the zip's header can lie; read_capped stops at the cap
        // whatever it says.
        if entry.size() > max {
            return Err(PackError::TooLarge {
                what: what.to_string(),
                max_bytes: max,
            });
        }
        read_capped(entry, max, what).map(Some)
    }
}

/// A path the profile names for an image: relative, inside the pack, with
/// no `..`, and a PNG or JPEG by its name.
pub fn image_path_ok(path: &str) -> Result<(), String> {
    if path.is_empty() || path.contains('\\') || path.contains('\0') {
        return Err("isn't a path inside the pack".into());
    }
    let p = Path::new(path);
    if !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("isn't a path inside the pack".into());
    }
    if path == PROFILE || path == HASHES {
        return Err("names one of the pack's own files".into());
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("png" | "jpg" | "jpeg") => Ok(()),
        _ => Err("must be a .png, .jpg or .jpeg file".into()),
    }
}

/// The width and height of a PNG or JPEG, or why it isn't one.
pub fn image_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // The first chunk is IHDR: width and height, big-endian.
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return Err("a damaged PNG".into());
        }
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Ok((w, h));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return jpeg_size(bytes).ok_or_else(|| "a damaged JPEG".to_string());
    }
    Err("not a PNG or JPEG".into())
}

/// Walks a JPEG's markers to the first start-of-frame, which holds its size.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        // Fill bytes, and markers with no length.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 {
            return None;
        }
        let is_frame = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame {
            let at = i + 5;
            if at + 4 > bytes.len() {
                return None;
            }
            let h = u16::from_be_bytes([bytes[at], bytes[at + 1]]) as u32;
            let w = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as u32;
            return Some((w, h));
        }
        i += 2 + len;
    }
    None
}

/// The images a profile names, by path, with what each is for.
pub fn named_images(profile: &Profile) -> Vec<(&'static str, &str)> {
    let mut out = Vec::new();
    if let Some(logo) = &profile.branding.logo {
        out.push(("logo", logo.value.as_str()));
    }
    if let Some(hero) = &profile.branding.hero {
        out.push(("hero image", hero.value.as_str()));
    }
    out
}

fn check(files: &mut dyn Files) -> Result<Pack, PackError> {
    let profile_json = files
        .get(PROFILE, MAX_PROFILE_BYTES, "profile.json")?
        .ok_or_else(|| PackError::NotAPack("no profile.json".into()))?;
    let text = std::str::from_utf8(&profile_json)
        .map_err(|_| PackError::Invalid(vec!["profile.json isn't UTF-8 text".into()]))?;
    let profile = Profile::from_json(text)
        .map_err(|problems| PackError::Invalid(problems.iter().map(|p| p.to_string()).collect()))?;

    let hashes: Option<BTreeMap<String, String>> = files
        .get(HASHES, MAX_PROFILE_BYTES, "hashes.json")?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|e| PackError::NotAPack(format!("hashes.json can't be read: {e}")))
        })
        .transpose()?;

    let mut images = BTreeMap::new();
    for (what, path) in named_images(&profile) {
        image_path_ok(path).map_err(|reason| PackError::BadImage {
            path: path.to_string(),
            reason: format!("the {what} {reason}"),
        })?;
        if images.contains_key(path) {
            continue;
        }
        let bytes = files
            .get(path, MAX_IMAGE_BYTES, &format!("the {what}"))?
            .ok_or_else(|| PackError::BadImage {
                path: path.to_string(),
                reason: format!("the profile names it as its {what}, but it isn't in the pack"),
            })?;
        let (w, h) = image_size(&bytes).map_err(|reason| PackError::BadImage {
            path: path.to_string(),
            reason: format!("the {what} is {reason}"),
        })?;
        if w == 0 || h == 0 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
            return Err(PackError::BadImage {
                path: path.to_string(),
                reason: format!(
                    "the {what} is {w} by {h} pixels; each side must be 1 to {MAX_IMAGE_SIDE}"
                ),
            });
        }
        images.insert(path.to_string(), bytes);
    }

    let pack = Pack {
        profile,
        profile_json,
        images,
    };
    if let Some(hashes) = hashes {
        for (path, bytes) in pack.files() {
            match hashes.get(&path) {
                Some(want) if want.eq_ignore_ascii_case(&sha256(bytes)) => {}
                _ => return Err(PackError::HashMismatch { path }),
            }
        }
    }
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths_stay_inside_the_pack() {
        assert!(image_path_ok("images/logo.png").is_ok());
        assert!(image_path_ok("hero.JPG").is_ok());
        for bad in [
            "../logo.png",
            "/etc/logo.png",
            "images/../../x.png",
            "logo.gif",
            "profile.json",
            "a\\b.png",
            "",
        ] {
            assert!(image_path_ok(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sizes_are_read_from_png_and_jpeg_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(image_size(&png), Ok((640, 480)));

        // SOI, an APP0 segment, then SOF0 with height 300 and width 400.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x01,
            0x2C, 0x01, 0x90, 0x01, 0x01, 0x11, 0x00,
        ];
        assert_eq!(image_size(&jpeg), Ok((400, 300)));
        assert!(image_size(&[0xFF, 0xD8, 0xFF, 0xE0]).is_err());
        assert!(image_size(b"GIF89a").is_err());
    }
}
