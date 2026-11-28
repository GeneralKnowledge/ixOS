//! The preview's rules, end to end through the core: importing packs (folder,
//! zip, lone profile), what is refused, what the screens may show, and the
//! copy kept on the phone.

use std::io::Write;
use std::path::{Path, PathBuf};

use business_preview_core::{
    business_preview_pack_address, BookKind, BusinessPreviewCore, BusinessPreviewError,
    ContactKind, CONCEPT_LABEL,
};
use forge_runtime::block_on;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn profile_text() -> String {
    std::fs::read_to_string(fixture().join("profile.json")).unwrap()
}

fn logo() -> Vec<u8> {
    std::fs::read(fixture().join("images/logo.png")).unwrap()
}

fn open(dir: &Path) -> std::sync::Arc<BusinessPreviewCore> {
    BusinessPreviewCore::new(dir.to_string_lossy().into_owned()).unwrap()
}

/// A pack folder with `profile` and the logo.
fn folder(root: &Path, profile: &str) -> PathBuf {
    let dir = root.join("pack");
    std::fs::create_dir_all(dir.join("images")).unwrap();
    std::fs::write(dir.join("profile.json"), profile).unwrap();
    std::fs::write(dir.join("images/logo.png"), logo()).unwrap();
    dir
}

fn zipped(root: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = root.join("pack.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn import(
    core: &BusinessPreviewCore,
    path: &Path,
) -> Result<business_preview_core::Imported, BusinessPreviewError> {
    block_on(core.import_pack(path.to_string_lossy().into_owned()))
}

#[test]
fn a_folder_pack_is_kept_and_shown() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    let imported = import(&core, &folder(work.path(), &profile_text())).unwrap();
    assert!(!imported.replaced);
    assert_eq!(imported.prospect.name, "The Riding School");
    assert_eq!(imported.prospect.category, "Riding school");
    assert!(imported.prospect.is_concept);
    // The unverified price is the one value left to check.
    assert_eq!(imported.prospect.unverified, 1);
    assert_eq!(core.prospects().len(), 1);

    // The pack the person sent can go: the phone keeps its own copy.
    std::fs::remove_dir_all(work.path().join("pack")).unwrap();
    let shown = block_on(core.business("riding-school-example".into())).unwrap();
    assert_eq!(shown.concept_label.as_deref(), Some(CONCEPT_LABEL));
    assert_eq!(shown.words.services_title, "Lessons");
    assert_eq!(shown.words.book_label, "Book a Lesson");
    assert_eq!(shown.primary_color.as_deref(), Some("#171717"));
    let logo_path = shown.logo_path.unwrap();
    assert_eq!(std::fs::read(&logo_path).unwrap(), logo());

    // Only the verified price is shown; the other is counted.
    assert_eq!(shown.services[0].name, "Private lesson (30 min)");
    assert_eq!(shown.services[0].price, None);
    assert_eq!(shown.services[0].duration.as_deref(), Some("30 min"));
    assert_eq!(shown.services[1].price.as_deref(), Some("£35.00"));
    assert_eq!(shown.hidden_prices, 1);

    assert_eq!(shown.hours.len(), 1);
    assert_eq!(shown.hours[0].day, "Saturday");
    assert_eq!(shown.hours[0].times, "09:00 to 17:00");

    let book = shown.book.unwrap();
    assert_eq!(book.kind, BookKind::Page);
    assert_eq!(book.url, "https://booking.example/riding-school");

    let kinds: Vec<_> = shown
        .contact
        .iter()
        .map(|c| (c.kind, c.url.as_str()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (ContactKind::Phone, "tel:01506000000"),
            (ContactKind::Email, "mailto:hello@example.com"),
            (
                ContactKind::Address,
                "https://maps.apple.com/?q=Example+Farm%2C+Livingston%2C+EH54+0AA"
            ),
            (ContactKind::Website, "https://www.facebook.com/example"),
        ]
    );
    assert_eq!(shown.contact[3].label, "facebook.com");
}

#[test]
fn prospects_survive_a_restart_and_a_second_import_replaces_the_first() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    import(&core, &folder(work.path(), &profile_text())).unwrap();
    core.shutdown();
    drop(core);

    let core = open(data.path());
    assert_eq!(core.prospects().len(), 1);
    let renamed = profile_text().replace("The Riding School", "The Riding School at Example Farm");
    let again = import(&core, &folder(work.path(), &renamed)).unwrap();
    assert!(again.replaced);
    let list = core.prospects();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "The Riding School at Example Farm");
}

#[test]
fn a_zipped_pack_folder_imports_and_leaves_the_extractors_extras_behind() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    let text = profile_text();
    let zip = zipped(
        work.path(),
        &[
            ("riding-school/profile.json", text.as_bytes()),
            ("riding-school/images/logo.png", &logo()),
            ("riding-school/review.txt", b"check services[0].price"),
            ("riding-school/pages/s1.txt", b"page text"),
        ],
    );
    import(&core, &zip).unwrap();
    let kept = data.path().join("packs/riding-school-example");
    assert!(kept.join("profile.json").is_file());
    assert!(kept.join("images/logo.png").is_file());
    assert!(!kept.join("pages").exists());
    assert!(!kept.join("review.txt").exists());
}

#[test]
fn a_lone_profile_without_images_imports() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    let no_logo = profile_text().replace(
        r#",
    "logo": { "value": "images/logo.png", "source": "s1", "method": "html", "verified": true }"#,
        "",
    );
    assert!(!no_logo.contains("logo"), "the fixture changed shape");
    let path = work.path().join("profile.json");
    std::fs::write(&path, no_logo).unwrap();
    import(&core, &path).unwrap();
    let shown = block_on(core.business("riding-school-example".into())).unwrap();
    assert_eq!(shown.logo_path, None);

    // A lone profile that names a logo is refused: the logo isn't there.
    std::fs::write(&path, profile_text()).unwrap();
    let err = import(&core, &path).unwrap_err();
    assert!(
        matches!(err, BusinessPreviewError::BadImage { .. }),
        "{err:?}"
    );
}

#[test]
fn invalid_profiles_and_bad_images_are_refused() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());

    // A live action (a booking the app would make) isn't allowed in version 0.
    let live = profile_text().replace(r#""kind": "link""#, r#""kind": "live""#);
    match import(&core, &folder(work.path(), &live)).unwrap_err() {
        BusinessPreviewError::Invalid { problems } => {
            assert!(
                problems.iter().any(|p| p.starts_with("actions.book")),
                "{problems:?}"
            )
        }
        other => panic!("{other:?}"),
    }

    // An approved preview needs the business's written consent.
    let approved = profile_text().replace(r#""status": "concept""#, r#""status": "approved""#);
    assert!(matches!(
        import(&core, &folder(work.path(), &approved)).unwrap_err(),
        BusinessPreviewError::Invalid { .. }
    ));

    // A logo that isn't a picture.
    let dir = folder(work.path(), &profile_text());
    std::fs::write(dir.join("images/logo.png"), b"<html>not a png</html>").unwrap();
    assert!(matches!(
        import(&core, &dir).unwrap_err(),
        BusinessPreviewError::BadImage { .. }
    ));

    // Something that isn't a pack at all.
    let junk = work.path().join("notes.txt");
    std::fs::write(&junk, "hello").unwrap();
    assert!(matches!(
        import(&core, &junk).unwrap_err(),
        BusinessPreviewError::Invalid { .. }
    ));

    assert!(core.prospects().is_empty());
}

#[test]
fn listed_hashes_must_match() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    let dir = folder(work.path(), &profile_text());
    let good = format!(
        r#"{{"profile.json": "{}", "images/logo.png": "{}"}}"#,
        business_preview_core::pack::sha256(profile_text().as_bytes()),
        business_preview_core::pack::sha256(&logo()),
    );
    std::fs::write(dir.join("hashes.json"), &good).unwrap();
    import(&core, &dir).unwrap();

    let wrong = good.replace(
        &business_preview_core::pack::sha256(&logo()),
        &"0".repeat(64),
    );
    std::fs::write(dir.join("hashes.json"), wrong).unwrap();
    assert_eq!(
        import(&core, &dir).unwrap_err(),
        BusinessPreviewError::HashMismatch {
            path: "images/logo.png".into()
        }
    );
}

#[test]
fn a_damaged_copy_is_never_shown() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    import(&core, &folder(work.path(), &profile_text())).unwrap();
    let kept = data.path().join("packs/riding-school-example/profile.json");
    let changed = std::fs::read_to_string(&kept)
        .unwrap()
        .replace("The Riding School", "Another Business");
    std::fs::write(&kept, changed).unwrap();
    assert_eq!(
        block_on(core.business("riding-school-example".into())).unwrap_err(),
        BusinessPreviewError::Damaged {
            id: "riding-school-example".into()
        }
    );
}

#[test]
fn delete_removes_the_prospect_and_its_files() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    import(&core, &folder(work.path(), &profile_text())).unwrap();
    block_on(core.delete("riding-school-example".into())).unwrap();
    assert!(core.prospects().is_empty());
    assert!(!data.path().join("packs/riding-school-example").exists());
    assert_eq!(
        block_on(core.delete("riding-school-example".into())).unwrap_err(),
        BusinessPreviewError::NotFound {
            id: "riding-school-example".into()
        }
    );
}

#[test]
fn two_businesses_never_share_pictures() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    import(&core, &folder(work.path(), &profile_text())).unwrap();
    let other = profile_text()
        .replace("riding-school-example", "salon-example")
        .replace("riding_school", "hair_and_beauty");
    import(&core, &folder(&work.path().join("b"), &other)).unwrap();
    let a = block_on(core.business("riding-school-example".into())).unwrap();
    let b = block_on(core.business("salon-example".into())).unwrap();
    assert_ne!(a.logo_path, b.logo_path);
    assert!(b.logo_path.unwrap().contains("salon-example"));
    assert_eq!(b.words.book_label, "Book an Appointment");
    assert_eq!(b.words.services_title, "Treatments");
}

#[test]
fn packs_are_downloaded_over_https_only() {
    assert_eq!(
        business_preview_pack_address(" https://example.com/packs/a.zip ".into()).unwrap(),
        "https://example.com/packs/a.zip"
    );
    for bad in [
        "http://example.com/a.zip",
        "example.com/a.zip",
        "file:///etc/passwd",
        "https://user:pw@example.com/a.zip",
    ] {
        assert!(
            matches!(
                business_preview_pack_address(bad.into()),
                Err(BusinessPreviewError::BadAddress { .. })
            ),
            "{bad}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_linked_folder_inside_a_pack_is_never_followed() {
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let core = open(data.path());
    let dir = folder(work.path(), &profile_text());
    // `images` becomes a link to a folder outside the pack that holds a real logo.
    let outside = work.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("logo.png"), logo()).unwrap();
    std::fs::remove_dir_all(dir.join("images")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("images")).unwrap();
    assert!(matches!(
        import(&core, &dir).unwrap_err(),
        BusinessPreviewError::NotAPack { .. }
    ));
    assert!(core.prospects().is_empty());
}
