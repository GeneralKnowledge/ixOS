use super::*;
use std::io::Write;

/// A wheel built here: `files` as (path, text), plus a .dist-info with
/// METADATA (`requires` as Requires-Dist lines), WHEEL and RECORD.
fn wheel(
    dir: &Path,
    name: &str,
    version: &str,
    files: &[(&str, &str)],
    requires: &[&str],
    pure: bool,
) -> (String, String) {
    let info = format!("{}-{version}.dist-info", name.replace('-', "_"));
    let path = dir.join(format!(
        "{}-{version}-py3-none-any.whl",
        name.replace('-', "_")
    ));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let mut record = String::new();
    for (file, text) in files {
        zip.start_file(*file, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
        record += &format!("{file},,\n");
    }
    let mut metadata = format!(
        "Metadata-Version: 2.1\nName: {name}\nVersion: {version}\nSummary: The {name} package\n"
    );
    for r in requires {
        metadata += &format!("Requires-Dist: {r}\n");
    }
    metadata += "\nA long description: Requires-Dist: not-a-header\n";
    let wheel_text = format!("Wheel-Version: 1.0\nRoot-Is-Purelib: {pure}\nTag: py3-none-any\n");
    for (file, text) in [("METADATA", metadata), ("WHEEL", wheel_text)] {
        zip.start_file(format!("{info}/{file}"), options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
        record += &format!("{info}/{file},,\n");
    }
    record += &format!("{info}/RECORD,,\n");
    zip.start_file(format!("{info}/RECORD"), options).unwrap();
    zip.write_all(record.as_bytes()).unwrap();
    zip.finish().unwrap();
    let sum = forge_storage::files::sha256_file(&path).unwrap();
    (path.display().to_string(), sum)
}

#[test]
fn names_are_compared_as_pypi_does() {
    assert_eq!(normalize("Typing_Extensions"), "typing-extensions");
    assert_eq!(normalize("zope.interface"), "zope-interface");
    assert_eq!(normalize("a__b--c"), "a-b-c");
    assert!(package_name_valid("python-dateutil".into()));
    assert!(!package_name_valid("../etc".into()));
    assert!(!package_name_valid("a b".into()));
    assert!(!package_name_valid("".into()));
}

#[test]
fn the_pure_wheel_of_the_newest_release_is_chosen() {
    let json = r#"{
      "info": {"name": "attrs", "version": "25.1.0"},
      "urls": [
        {"filename": "attrs-25.1.0.tar.gz", "packagetype": "sdist", "url": "https://files/attrs.tar.gz", "digests": {"sha256": "AA"}, "size": 10},
        {"filename": "attrs-25.1.0-py3-none-any.whl", "packagetype": "bdist_wheel", "url": "https://files/attrs.whl", "digests": {"sha256": "ABCD"}, "size": 63000, "yanked": false}
      ]}"#;
    let choice = package_choose_wheel("attrs".into(), json.into()).unwrap();
    assert_eq!(choice.filename, "attrs-25.1.0-py3-none-any.whl");
    assert_eq!(choice.sha256, "abcd");
    assert_eq!(choice.version, "25.1.0");

    let compiled = r#"{
      "info": {"name": "numpy", "version": "2.3.0"},
      "urls": [
        {"filename": "numpy-2.3.0-cp313-cp313-macosx_14_0_arm64.whl", "packagetype": "bdist_wheel", "url": "https://files/n.whl", "digests": {"sha256": "AA"}, "size": 10}
      ]}"#;
    let err = package_choose_wheel("numpy".into(), compiled.into()).unwrap_err();
    assert!(matches!(err, PackageError::NotPure { .. }), "{err}");
    assert!(err.to_string().contains("compiled"));

    assert!(matches!(
        package_choose_wheel("nope".into(), r#"{"message": "Not Found"}"#.into()),
        Err(PackageError::NotFound { .. })
    ));
    assert!(is_pure_wheel("six-1.17.0-py2.py3-none-any.whl"));
    assert!(!is_pure_wheel(
        "x-1.0-cp315-abi3-ios_13_0_arm64_iphoneos.whl"
    ));
}

#[test]
fn a_wheel_installs_lists_and_uninstalls() {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    let (path, sum) = wheel(
        dir.path(),
        "tiny-pkg",
        "1.0",
        &[
            ("tiny_pkg/__init__.py", "X = 1\n"),
            ("tiny_pkg/sub/m.py", "Y = 2\n"),
        ],
        &[
            "six>=1.0",
            "colorama; sys_platform == \"win32\"",
            "rich[jupyter]>=10; extra == \"fancy\"",
            "tomli>=1.1; python_version < \"3.11\"",
            "exceptiongroup; python_version >= \"3.8\"",
        ],
        true,
    );
    let site_text = site.display().to_string();
    let installed = package_install_wheel(path, sum, site_text.clone()).unwrap();
    assert_eq!(installed.name, "tiny-pkg");
    assert_eq!(installed.requires, ["six", "exceptiongroup"]);
    assert_eq!(
        fs::read_to_string(site.join("tiny_pkg/sub/m.py")).unwrap(),
        "Y = 2\n"
    );
    assert_eq!(packages_installed(site_text.clone()).len(), 1);
    // No staging folder left behind.
    assert_eq!(fs::read_dir(&site).unwrap().count(), 2);

    // A newer version replaces the old one's files.
    let (path, sum) = wheel(
        dir.path(),
        "tiny-pkg",
        "2.0",
        &[("tiny_pkg/__init__.py", "X = 2\n")],
        &[],
        true,
    );
    package_install_wheel(path, sum, site_text.clone()).unwrap();
    assert!(!site.join("tiny_pkg/sub/m.py").exists());
    let list = packages_installed(site_text.clone());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].version, "2.0");

    package_uninstall(site_text.clone(), "Tiny_Pkg".into()).unwrap();
    assert_eq!(fs::read_dir(&site).unwrap().count(), 0);
    assert!(matches!(
        package_uninstall(site_text, "tiny-pkg".into()),
        Err(PackageError::NotFound { .. })
    ));
}

#[test]
fn a_bad_checksum_compiled_code_or_anothers_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    let site_text = site.display().to_string();
    let (path, _) = wheel(dir.path(), "a", "1.0", &[("a/__init__.py", "")], &[], true);
    let err = package_install_wheel(path, "00".repeat(32), site_text.clone()).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");

    let (path, sum) = wheel(
        dir.path(),
        "fast",
        "1.0",
        &[("fast/_speed.cpython-315-iphoneos.so", "\x7fELF")],
        &[],
        true,
    );
    assert!(matches!(
        package_install_wheel(path, sum, site_text.clone()),
        Err(PackageError::NotPure { .. })
    ));
    let (path, sum) = wheel(
        dir.path(),
        "plat",
        "1.0",
        &[("plat/__init__.py", "")],
        &[],
        false,
    );
    assert!(matches!(
        package_install_wheel(path, sum, site_text.clone()),
        Err(PackageError::NotPure { .. })
    ));

    let (path, sum) = wheel(
        dir.path(),
        "owner",
        "1.0",
        &[("shared/__init__.py", "A\n")],
        &[],
        true,
    );
    package_install_wheel(path, sum, site_text.clone()).unwrap();
    let (path, sum) = wheel(
        dir.path(),
        "intruder",
        "1.0",
        &[("shared/__init__.py", "B\n")],
        &[],
        true,
    );
    let err = package_install_wheel(path, sum, site_text).unwrap_err();
    assert!(err.to_string().contains("another package"), "{err}");
    assert_eq!(
        fs::read_to_string(site.join("shared/__init__.py")).unwrap(),
        "A\n"
    );
    assert!(!site.read_dir().unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".installing")));
}

#[test]
fn markers_are_read_for_cpython_on_ios() {
    assert!(marker_true("python_version >= \"3.8\""));
    assert!(!marker_true("python_version < \"3.11\""));
    assert!(marker_true("python_version == \"3.*\""));
    assert!(!marker_true("sys_platform == \"win32\""));
    assert!(marker_true(
        "sys_platform != \"win32\" and implementation_name == \"cpython\""
    ));
    assert!(marker_true(
        "(os_name == \"nt\" or os_name == \"posix\") and python_full_version >= \"3.9.1\""
    ));
    assert!(!marker_true("extra == \"test\""));
    assert!(marker_true("'ios' in sys_platform"));
    assert!(!marker_true("platform_machine == \"x86_64\""));
    // Unreadable markers leave the package out.
    assert!(!marker_true("python_version >"));
    assert!(!marker_true("unknown_var == \"x\""));
}

/// Real packages from PyPI, when `PYPI_DIR` holds `<name>.json` and the
/// wheels it names (downloaded beforehand; the tests themselves never go
/// online). Installs each and its requirements' wheels found there.
#[test]
#[ignore]
fn real_pypi_packages_install() {
    let Ok(dir) = std::env::var("PYPI_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    let site = dir.join("site");
    let _ = fs::remove_dir_all(&site);
    let mut queue: Vec<String> = std::env::var("PYPI_NAMES")
        .unwrap()
        .split(',')
        .map(str::to_string)
        .collect();
    let mut done = BTreeSet::new();
    while let Some(name) = queue.pop() {
        if !done.insert(normalize(&name)) {
            continue;
        }
        let json = fs::read_to_string(dir.join(format!("{}.json", normalize(&name)))).unwrap();
        let choice = package_choose_wheel(name.clone(), json).unwrap();
        let wheel = dir.join(&choice.filename);
        let installed = package_install_wheel(
            wheel.display().to_string(),
            choice.sha256,
            site.display().to_string(),
        )
        .unwrap();
        println!(
            "{} {} needs {:?}",
            installed.name, installed.version, installed.requires
        );
        queue.extend(installed.requires);
    }
}
