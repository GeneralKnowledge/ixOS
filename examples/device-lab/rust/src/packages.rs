//! Pure-Python packages for Python Lab (python-lab-workspace-plan.md, PL2).
//!
//! There is no pip in the app (prepare-python.sh removes `ensurepip`), and
//! iOS loads no compiled code that wasn't signed into the app. So a package
//! here is a **pure-Python wheel** from PyPI: a zip of `.py` files. Swift
//! downloads PyPI's JSON for a name and the wheel; this module chooses the
//! wheel, checks its SHA-256 against PyPI's, refuses anything with compiled
//! code, unpacks it into the user `site-packages` folder (never over
//! another package's files) and says which requirements to install next.
//! Compiled packages (numpy and the like) can only come built into the app
//! (scripts/prepare-python.sh, docs/numpy-spike.md).

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The Python the app embeds, for `python_version` in requirement markers.
pub const PYTHON_VERSION: &str = "3.15";
/// The largest wheel installed (a pure-Python package is rarely over a few
/// MB; this stops a mistaken name downloading something huge).
pub const WHEEL_MAX_BYTES: u64 = 50 * 1024 * 1024;
/// The most a wheel may unpack to.
const UNPACKED_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// The most a wheel's WHEEL or METADATA file may unpack to.
const METADATA_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error, thiserror::Error)]
pub enum PackageError {
    #[error("PyPI has no package called {name}.")]
    NotFound { name: String },
    /// It needs compiled code, which iOS only runs when built into the app.
    #[error("{reason}")]
    NotPure { reason: String },
    #[error("{reason}")]
    Invalid { reason: String },
    #[error("{reason}")]
    Storage { reason: String },
}

fn invalid(reason: impl Into<String>) -> PackageError {
    PackageError::Invalid {
        reason: reason.into(),
    }
}

fn storage(err: impl std::fmt::Display) -> PackageError {
    PackageError::Storage {
        reason: err.to_string(),
    }
}

/// The wheel to download for a package.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WheelChoice {
    pub name: String,
    pub version: String,
    pub filename: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

/// A package in a `site-packages` folder.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub summary: String,
    /// The packages it needs on this phone (markers evaluated, extras left
    /// out), by name.
    pub requires: Vec<String>,
}

/// A package name as PyPI compares them (PEP 503): lowercase, with runs of
/// `-`, `_` and `.` as one `-`.
#[uniffi::export]
pub fn package_normalized_name(name: String) -> String {
    normalize(&name)
}

fn normalize(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            dash = true;
        } else {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Whether `name` could be a PyPI name, so nothing odd goes into a URL.
#[uniffi::export]
pub fn package_name_valid(name: String) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

/// Chooses the pure-Python wheel of the newest release from PyPI's JSON
/// for a package (`https://pypi.org/pypi/<name>/json`).
#[uniffi::export]
pub fn package_choose_wheel(name: String, pypi_json: String) -> Result<WheelChoice, PackageError> {
    let json: serde_json::Value = serde_json::from_str(&pypi_json)
        .map_err(|_| PackageError::NotFound { name: name.clone() })?;
    let info = &json["info"];
    let Some(project) = info["name"].as_str() else {
        return Err(PackageError::NotFound { name });
    };
    let version = info["version"].as_str().unwrap_or_default().to_string();
    let files = json["urls"].as_array().cloned().unwrap_or_default();
    let mut has_wheel = false;
    let mut needs_newer: Option<String> = None;
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or_default();
        if file["packagetype"].as_str() != Some("bdist_wheel") || file["yanked"] == true {
            continue;
        }
        has_wheel = true;
        if !is_pure_wheel(filename) {
            continue;
        }
        // The file's own Requires-Python, else the release's.
        let requires = file["requires_python"]
            .as_str()
            .or_else(|| info["requires_python"].as_str())
            .unwrap_or_default();
        if !python_allowed(requires) {
            needs_newer = Some(requires.to_string());
            continue;
        }
        let Some(sha256) = file["digests"]["sha256"].as_str() else {
            continue;
        };
        let bytes = file["size"].as_u64().unwrap_or(0);
        if bytes > WHEEL_MAX_BYTES {
            return Err(invalid(format!(
                "{project} {version} is {} MB; a package can be at most {} MB.",
                bytes / (1024 * 1024),
                WHEEL_MAX_BYTES / (1024 * 1024)
            )));
        }
        let url = file["url"].as_str().unwrap_or_default();
        if !url.starts_with("https://") {
            continue;
        }
        return Ok(WheelChoice {
            name: project.to_string(),
            version,
            filename: filename.to_string(),
            url: url.to_string(),
            sha256: sha256.to_lowercase(),
            bytes,
        });
    }
    if let Some(requires) = needs_newer {
        return Err(invalid(format!(
            "{project} {version} needs Python {requires}, and this app has Python {PYTHON_VERSION}."
        )));
    }
    Err(PackageError::NotPure {
        reason: if has_wheel {
            format!(
                "{project} {version} has only compiled wheels. iOS runs compiled code only when it is built into the app, so it can't be installed here."
            )
        } else {
            format!(
                "{project} {version} has no wheel, only source to build, which the app can't do. Look for a pure-Python alternative."
            )
        },
    })
}

/// Whether this app's Python meets a `Requires-Python` specifier such as
/// `>=3.9,<4` (empty, or a clause it can't read, allows it).
pub(crate) fn python_allowed(spec: &str) -> bool {
    const OPERATORS: [&str; 8] = ["===", "~=", "==", "!=", "<=", ">=", "<", ">"];
    spec.split(',')
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .all(|clause| {
            let Some(op) = OPERATORS.iter().find(|op| clause.starts_with(**op)) else {
                return true;
            };
            // A pre-, post- or dev-release counts as its release
            // (`>=3.16rc1` as `>=3.16`); one that still can't be read
            // excludes the wheel rather than risk it.
            let version = clause[op.len()..].trim();
            let release = version
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '*'))
                .map_or(version, |end| &version[..end])
                .trim_end_matches('.');
            compare_versions(PYTHON_VERSION, op, release).unwrap_or(false)
        })
}

/// `name-version(-build)?-python-abi-platform.whl` with the platform `any`,
/// the ABI `none` and a Python 3 tag.
fn is_pure_wheel(filename: &str) -> bool {
    let Some(stem) = filename.strip_suffix(".whl") else {
        return false;
    };
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return false;
    }
    let n = parts.len();
    let (python, abi, platform) = (parts[n - 3], parts[n - 2], parts[n - 1]);
    platform == "any" && abi == "none" && python.split('.').any(|t| t == "py3" || t == "py30")
}

/// Installs the wheel at `wheel` into `site_packages` once its SHA-256 is
/// `sha256` (PyPI's), and returns what it installed. An older version of
/// the same package is replaced; another package's files never are.
#[uniffi::export]
pub fn package_install_wheel(
    wheel: String,
    sha256: String,
    site_packages: String,
) -> Result<InstalledPackage, PackageError> {
    let wheel = Path::new(&wheel);
    let site = Path::new(&site_packages);
    let size = fs::metadata(wheel).map_err(storage)?.len();
    if size > WHEEL_MAX_BYTES {
        return Err(invalid("That package is too large."));
    }
    let actual = forge_storage::files::sha256_file(wheel).map_err(storage)?;
    if !actual.eq_ignore_ascii_case(sha256.trim()) {
        return Err(invalid(
            "The download doesn't match PyPI's checksum, so it wasn't installed. Try again.",
        ));
    }
    let file = fs::File::open(wheel).map_err(storage)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|_| invalid("That isn't a wheel (a zip file)."))?;

    // Find the .dist-info and check the wheel says it's pure.
    let names: Vec<String> = zip
        .file_names()
        .filter_map(Result::ok)
        .map(|n| n.into_owned())
        .collect();
    let dist_info = names
        .iter()
        .filter_map(|n| n.split('/').next())
        .find(|top| top.ends_with(".dist-info"))
        .map(str::to_string)
        .ok_or_else(|| invalid("That wheel has no .dist-info folder."))?;
    let wheel_meta = read_entry(&mut zip, &format!("{dist_info}/WHEEL"))?;
    if !header(&wheel_meta, "Root-Is-Purelib")
        .first()
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return Err(PackageError::NotPure {
            reason: "That wheel isn't pure Python (Root-Is-Purelib is not true).".into(),
        });
    }
    let metadata = read_entry(&mut zip, &format!("{dist_info}/METADATA"))?;
    let package = describe(&metadata);
    let data = dist_info.replace(".dist-info", ".data");

    // Where each entry goes, refusing compiled code and odd paths.
    let mut plan: Vec<(usize, PathBuf)> = Vec::new();
    let mut tops: BTreeSet<String> = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| invalid(e.to_string()))?;
        let Some(path) = entry.enclosed_name() else {
            return Err(invalid(format!(
                "{} points outside the package.",
                entry.name().map(|n| n.into_owned()).unwrap_or_default()
            )));
        };
        if entry.is_dir() {
            continue;
        }
        total += entry.size();
        if total > UNPACKED_MAX_BYTES {
            return Err(invalid("That package unpacks to more than 200 MB."));
        }
        let text = path.to_string_lossy().replace('\\', "/");
        let lower = text.to_lowercase();
        if [".so", ".dylib", ".pyd", ".dll"]
            .iter()
            .any(|ext| lower.ends_with(ext))
            || lower.contains(".so.")
        {
            return Err(PackageError::NotPure {
                reason: format!(
                    "{} {} contains compiled code ({text}).",
                    package.name, package.version
                ),
            });
        }
        let target = if let Some(rest) = text.strip_prefix(&format!("{data}/")) {
            match rest.split_once('/') {
                Some(("purelib", inner)) => inner.to_string(),
                Some(("platlib", _)) => {
                    return Err(PackageError::NotPure {
                        reason: format!(
                            "{} {} has platform-specific files.",
                            package.name, package.version
                        ),
                    })
                }
                // Scripts, headers and data files have no place on a phone.
                _ => continue,
            }
        } else {
            text.clone()
        };
        // Checked again after `.data/purelib/` comes off: `enclosed_name`
        // counted those two parts, so `..` in the rest could climb out.
        let target = PathBuf::from(target);
        if target.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return Err(invalid(format!("{text} points outside the package.")));
        }
        let target = target.to_string_lossy().replace('\\', "/");
        let target = target.trim_start_matches("./").to_string();
        // RECORD is comma- and line-separated; a name with either would
        // let one entry pose as another.
        if target.contains([',', '\r', '\n']) {
            return Err(invalid(format!(
                "{} {} has a file name RECORD can't hold.",
                package.name, package.version
            )));
        }
        if let Some(top) = target.split('/').next() {
            tops.insert(top.to_string());
        }
        plan.push((index, PathBuf::from(target)));
    }

    // An installed version of this package goes first; anything else
    // already there belongs to another package and stays.
    fs::create_dir_all(site).map_err(storage)?;
    let old = installed_dist_infos(site)
        .into_iter()
        .find(|(_, p)| normalize(&p.name) == normalize(&package.name));
    let mut owned_by_old: BTreeSet<String> = BTreeSet::new();
    let mut old_files: BTreeSet<String> = BTreeSet::new();
    if let Some((old_info, _)) = &old {
        // Never what another package lists (an older build kept the
        // wheel's own RECORD, which could claim anything).
        let others = other_tops(site, old_info);
        owned_by_old = record_tops(site, old_info)
            .into_iter()
            .filter(|top| !others.contains(top))
            .collect();
        owned_by_old.insert(old_info.clone());
        old_files = record_paths(site, old_info);
    }
    for top in &tops {
        if site.join(top).exists() && !owned_by_old.contains(top) {
            return Err(invalid(format!(
                "{} would replace {top}, which another package installed, so it wasn't installed.",
                package.name
            )));
        }
    }

    // Unpack beside, then move into place.
    let staging = site.join(format!(
        ".installing-{}-{}",
        normalize(&package.name),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&staging);
    let unpacked = (|| {
        for (index, target) in &plan {
            let mut entry = zip.by_index(*index).map_err(|e| invalid(e.to_string()))?;
            let out = staging.join(target);
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes).map_err(storage)?;
            fs::write(&out, bytes).map_err(storage)?;
        }
        Ok::<(), PackageError>(())
    })();
    // The installer's own RECORD: the files it put in place, where they
    // went (a `.data/purelib` file under its final path). Uninstalling
    // removes these and nothing a wheel's own RECORD claims.
    let record: String = plan
        .iter()
        .map(|(_, target)| format!("{},,\n", target.to_string_lossy()))
        .chain(std::iter::once(format!("{dist_info}/RECORD,,\n")))
        .collect();
    let unpacked = unpacked
        .and_then(|_| fs::write(staging.join(&dist_info).join("RECORD"), record).map_err(storage));
    if let Err(err) = unpacked {
        let _ = fs::remove_dir_all(&staging);
        return Err(err);
    }
    // The old version moves aside, not away, until the new one is in place.
    let backup = site.join(format!(
        ".replaced-{}-{}",
        normalize(&package.name),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&backup);
    let mut set_aside: Vec<String> = Vec::new();
    let mut placed: Vec<String> = Vec::new();
    let published = (|| {
        if !owned_by_old.is_empty() {
            fs::create_dir_all(&backup).map_err(storage)?;
        }
        for top in &owned_by_old {
            if site.join(top).exists() {
                fs::rename(site.join(top), backup.join(top)).map_err(storage)?;
                set_aside.push(top.clone());
            }
        }
        for top in &tops {
            fs::rename(staging.join(top), site.join(top)).map_err(storage)?;
            placed.push(top.clone());
        }
        // Files the old version didn't install (a package's own saved
        // data) are copied into the new one, unless it brings a file of
        // that name. Copied, so the backup stays whole if this fails.
        for top in &set_aside {
            if !top.ends_with(".dist-info") {
                keep_unrecorded(&backup, site, Path::new(top), &old_files).map_err(|err| {
                    invalid(format!(
                        "{} couldn't keep the files {} saved ({err}), so it wasn't upgraded.",
                        package.name, top
                    ))
                })?;
            }
        }
        Ok::<(), PackageError>(())
    })();
    if let Err(err) = published {
        // Back as it was: the new files out, the old ones in.
        for top in &placed {
            let path = site.join(top);
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
        let mut restored = true;
        for top in &set_aside {
            // A folder the new version didn't have, made only to hold
            // copied saved files: the originals are in the backup.
            let path = site.join(top);
            if !placed.contains(top) && path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            }
            restored &= fs::rename(backup.join(top), &path).is_ok();
        }
        let _ = fs::remove_dir_all(&staging);
        // Kept if anything couldn't go back, so nothing is lost.
        if restored {
            let _ = fs::remove_dir_all(&backup);
        }
        return Err(err);
    }
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);
    Ok(package)
}

/// Copies each file under `backup/rel` that `recorded` doesn't list (and
/// isn't a Python cache) to the same place under `site`. Fails if the new
/// version put something there.
fn keep_unrecorded(
    backup: &Path,
    site: &Path,
    rel: &Path,
    recorded: &BTreeSet<String>,
) -> std::io::Result<()> {
    let from = backup.join(rel);
    let meta = fs::symlink_metadata(&from)?;
    if meta.is_dir() {
        if rel.file_name().is_some_and(|n| n == "__pycache__") {
            return Ok(());
        }
        for entry in fs::read_dir(&from)? {
            keep_unrecorded(backup, site, &rel.join(entry?.file_name()), recorded)?;
        }
        return Ok(());
    }
    let key = rel.to_string_lossy().replace('\\', "/");
    let to = site.join(rel);
    if !meta.is_file() || recorded.contains(&key) {
        return Ok(());
    }
    if fs::symlink_metadata(&to).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("the new version has its own {key}"),
        ));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&from, &to)?;
    Ok(())
}

/// The packages in `site_packages`, by name.
#[uniffi::export]
pub fn packages_installed(site_packages: String) -> Vec<InstalledPackage> {
    let mut out: Vec<InstalledPackage> = installed_dist_infos(Path::new(&site_packages))
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    out.sort_by_key(|p| normalize(&p.name));
    out
}

/// Removes a package's files (as its RECORD lists them) and its
/// `.dist-info`.
#[uniffi::export]
pub fn package_uninstall(site_packages: String, name: String) -> Result<(), PackageError> {
    let site = Path::new(&site_packages);
    let Some((info, _)) = installed_dist_infos(site)
        .into_iter()
        .find(|(_, p)| normalize(&p.name) == normalize(&name))
    else {
        return Err(PackageError::NotFound { name });
    };
    remove_package(site, &info)
}

/// Each `.dist-info` folder in `site` with what its METADATA says.
fn installed_dist_infos(site: &Path) -> Vec<(String, InstalledPackage)> {
    let Ok(read) = fs::read_dir(site) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".dist-info") {
                return None;
            }
            let metadata = fs::read_to_string(entry.path().join("METADATA")).ok()?;
            Some((name, describe(&metadata)))
        })
        .collect()
}

/// The paths a package's RECORD lists.
fn record_paths(site: &Path, dist_info: &str) -> BTreeSet<String> {
    record_entries(site, dist_info).into_iter().collect()
}

/// The paths a package's RECORD lists, as plain relative paths inside
/// site-packages (`./` parts dropped, so `./keeper/x` is `keeper/x`);
/// anything absolute, empty or climbing out is left out.
fn record_entries(site: &Path, dist_info: &str) -> Vec<String> {
    fs::read_to_string(site.join(dist_info).join("RECORD"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split(',').next())
        .filter_map(|path| {
            if path.starts_with('/') || path.contains('\\') {
                return None;
            }
            let parts: Vec<&str> = path.split('/').filter(|p| *p != ".").collect();
            if parts.is_empty() || parts.iter().any(|p| p.is_empty() || *p == "..") {
                return None;
            }
            Some(parts.join("/"))
        })
        .collect()
}

/// The top-level names every installed package but `dist_info` lists,
/// and their `.dist-info` folders.
fn other_tops(site: &Path, dist_info: &str) -> BTreeSet<String> {
    installed_dist_infos(site)
        .into_iter()
        .filter(|(info, _)| info != dist_info)
        .flat_map(|(info, _)| {
            let mut tops = record_tops(site, &info);
            tops.insert(info);
            tops
        })
        .collect()
}

/// The top-level names a package's RECORD lists.
fn record_tops(site: &Path, dist_info: &str) -> BTreeSet<String> {
    record_entries(site, dist_info)
        .into_iter()
        .filter_map(|path| path.split('/').next().map(str::to_string))
        .collect()
}

fn remove_package(site: &Path, dist_info: &str) -> Result<(), PackageError> {
    // What other packages list is theirs, whatever this RECORD claims (one
    // installed by an older build carries the wheel's own RECORD).
    let others = other_tops(site, dist_info);
    let mut folders: BTreeSet<PathBuf> = BTreeSet::new();
    // Only plain relative paths inside site-packages.
    for path in record_entries(site, dist_info) {
        if path
            .split('/')
            .next()
            .is_some_and(|top| others.contains(top))
        {
            continue;
        }
        let full = site.join(&path);
        let _ = fs::remove_file(&full);
        let mut parent = full.parent();
        while let Some(dir) = parent {
            if dir == site {
                break;
            }
            folders.insert(dir.to_path_buf());
            parent = dir.parent();
        }
        // Python's own caches beside the file.
        if let (Some(dir), Some(stem)) = (full.parent(), full.file_stem()) {
            let cache = dir.join("__pycache__");
            if let Ok(read) = fs::read_dir(&cache) {
                let prefix = format!("{}.", stem.to_string_lossy());
                for entry in read.filter_map(Result::ok) {
                    if entry.file_name().to_string_lossy().starts_with(&prefix) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
                folders.insert(cache);
            }
        }
    }
    // Deepest first, and only once empty.
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
    for folder in folders {
        let _ = fs::remove_dir(folder);
    }
    let info = site.join(dist_info);
    if info.exists() {
        fs::remove_dir_all(info).map_err(storage)?;
    }
    Ok(())
}

fn read_entry(zip: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<String, PackageError> {
    let mut entry = zip
        .by_name(name)
        .map_err(|_| invalid(format!("That wheel has no {name}.")))?;
    // Read at most a little over the limit, so a wheel whose metadata
    // unpacks to gigabytes is refused before it fills memory.
    let mut text = String::new();
    (&mut entry)
        .take(METADATA_MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| invalid(format!("{name} isn't text.")))?;
    if text.len() as u64 > METADATA_MAX_BYTES {
        return Err(invalid(format!(
            "{name} is larger than a package's metadata can be."
        )));
    }
    Ok(text)
}

/// The values of `key:` lines in an email-style header block (stops at the
/// first blank line, where the description begins).
fn header(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_once(':'))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        .map(|(_, v)| v.trim().to_string())
        .collect()
}

fn describe(metadata: &str) -> InstalledPackage {
    let first = |key: &str| header(metadata, key).into_iter().next().unwrap_or_default();
    let mut requires: Vec<String> = Vec::new();
    for requirement in header(metadata, "Requires-Dist") {
        if let Some(name) = needed(&requirement) {
            if !requires.iter().any(|r| normalize(r) == normalize(&name)) {
                requires.push(name);
            }
        }
    }
    InstalledPackage {
        name: first("Name"),
        version: first("Version"),
        summary: first("Summary"),
        requires,
    }
}

/// The package a `Requires-Dist` value needs on this phone, or None when
/// its marker leaves it out (an extra, another platform or Python).
fn needed(requirement: &str) -> Option<String> {
    let (spec, marker) = match requirement.split_once(';') {
        Some((spec, marker)) => (spec, Some(marker.trim())),
        None => (requirement, None),
    };
    let name: String = spec
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if name.is_empty() {
        return None;
    }
    match marker {
        None => Some(name),
        Some(marker) => marker_true(marker).then_some(name),
    }
}

/// Evaluates a PEP 508 marker for CPython on iOS. A marker it can't read is
/// taken as false, so an unneeded package is left out rather than a wrong
/// one put in.
pub(crate) fn marker_true(marker: &str) -> bool {
    let tokens = marker_tokens(marker);
    let mut parser = Marker { tokens, at: 0 };
    let value = parser.or();
    value.unwrap_or(false) && parser.at == parser.tokens.len()
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Op(String),
    Open,
    Close,
}

fn marker_tokens(marker: &str) -> Vec<Token> {
    let chars: Vec<char> = marker.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' {
            out.push(Token::Open);
            i += 1;
        } else if c == ')' {
            out.push(Token::Close);
            i += 1;
        } else if c == '"' || c == '\'' {
            let end = chars[i + 1..]
                .iter()
                .position(|&d| d == c)
                .map(|p| i + 1 + p);
            let Some(end) = end else {
                return vec![Token::Op("?".into())];
            };
            out.push(Token::Text(chars[i + 1..end].iter().collect()));
            i = end + 1;
        } else if "<>=!~".contains(c) {
            let mut op = String::new();
            while i < chars.len() && "<>=!~".contains(chars[i]) {
                op.push(chars[i]);
                i += 1;
            }
            out.push(Token::Op(op));
        } else {
            let mut word = String::new();
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                word.push(chars[i]);
                i += 1;
            }
            if word.is_empty() {
                return vec![Token::Op("?".into())];
            }
            out.push(Token::Word(word));
        }
    }
    out
}

struct Marker {
    tokens: Vec<Token>,
    at: usize,
}

impl Marker {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn or(&mut self) -> Option<bool> {
        let mut value = self.and()?;
        while self.peek() == Some(&Token::Word("or".into())) {
            self.at += 1;
            let right = self.and()?;
            value = value || right;
        }
        Some(value)
    }

    fn and(&mut self) -> Option<bool> {
        let mut value = self.atom()?;
        while self.peek() == Some(&Token::Word("and".into())) {
            self.at += 1;
            let right = self.atom()?;
            value = value && right;
        }
        Some(value)
    }

    fn atom(&mut self) -> Option<bool> {
        if self.peek() == Some(&Token::Open) {
            self.at += 1;
            let value = self.or()?;
            if self.peek() != Some(&Token::Close) {
                return None;
            }
            self.at += 1;
            return Some(value);
        }
        let left = self.value()?;
        let op = match self.tokens.get(self.at)? {
            Token::Op(op) => op.clone(),
            Token::Word(w) if w == "in" => "in".to_string(),
            Token::Word(w) if w == "not" => {
                self.at += 1;
                match self.peek() {
                    Some(Token::Word(w)) if w == "in" => "not in".to_string(),
                    _ => return None,
                }
            }
            _ => return None,
        };
        self.at += 1;
        let right = self.value()?;
        compare(&left, &op, &right)
    }

    /// A variable's value on this phone, or a quoted string. An `extra`
    /// is never set (extras aren't installed), so it is None.
    fn value(&mut self) -> Option<Value> {
        let token = self.tokens.get(self.at)?.clone();
        self.at += 1;
        match token {
            Token::Text(text) => Some(Value::Text(text)),
            Token::Word(word) => {
                let text = match word.as_str() {
                    "python_version" => PYTHON_VERSION.to_string(),
                    "python_full_version" => format!("{PYTHON_VERSION}.0"),
                    "sys_platform" => "ios".into(),
                    "platform_system" => "iOS".into(),
                    "os_name" => "posix".into(),
                    "implementation_name" => "cpython".into(),
                    "platform_python_implementation" => "CPython".into(),
                    "platform_machine" => "arm64".into(),
                    "extra" => return Some(Value::Extra),
                    _ => return None,
                };
                let version = word.contains("version");
                Some(if version {
                    Value::Version(text)
                } else {
                    Value::Text(text)
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Value {
    Text(String),
    Version(String),
    Extra,
}

fn compare(left: &Value, op: &str, right: &Value) -> Option<bool> {
    use Value::*;
    match (left, right) {
        // Extras are never installed: `extra == "x"` is false.
        (Extra, _) | (_, Extra) => Some(op == "!=" || op == "not in"),
        (Version(v), Text(t)) => compare_versions(v, op, t),
        (Text(t), Version(v)) => compare_versions(t, op, v),
        (Text(a), Text(b)) | (Version(a), Version(b)) => match op {
            "==" | "===" => Some(a == b),
            "!=" => Some(a != b),
            "in" => Some(b.contains(a.as_str())),
            "not in" => Some(!b.contains(a.as_str())),
            _ => None,
        },
    }
}

fn compare_versions(a: &str, op: &str, b: &str) -> Option<bool> {
    let parse = |v: &str| -> Option<Vec<u64>> {
        v.trim()
            .trim_end_matches(".*")
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    let (x, y) = (parse(a)?, parse(b)?);
    let len = x.len().max(y.len());
    let pad = |v: Vec<u64>| -> Vec<u64> {
        v.into_iter()
            .chain(std::iter::repeat(0))
            .take(len)
            .collect()
    };
    let order = pad(x.clone()).cmp(&pad(y.clone()));
    use std::cmp::Ordering::*;
    Some(match op {
        "==" | "===" => {
            if b.ends_with(".*") {
                x.starts_with(&y)
            } else {
                order == Equal
            }
        }
        "!=" => {
            if b.ends_with(".*") {
                !x.starts_with(&y)
            } else {
                order != Equal
            }
        }
        "<" => order == Less,
        "<=" => order != Greater,
        ">" => order == Greater,
        ">=" => order != Less,
        // Compatible release: >= it and the same up to its last part.
        "~=" => order != Less && y.len() >= 2 && x.starts_with(&y[..y.len() - 1]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
