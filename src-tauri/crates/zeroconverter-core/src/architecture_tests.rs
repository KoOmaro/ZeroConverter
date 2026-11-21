//! Rules that keep the engine reusable (the app and the CLI share it) and
//! offline, checked on the source so a slip fails the build, not a review.
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

fn core_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Lines of code, without comments, so a doc comment may name things.
fn code(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn core_does_not_depend_on_tauri() {
    let offenders: Vec<_> = rust_files(&core_dir())
        .into_iter()
        .filter(|p| !p.ends_with("architecture_tests.rs"))
        .filter(|p| code(p).contains("tauri::"))
        .collect();
    assert!(offenders.is_empty(), "tauri used in core: {offenders:?}");
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(
        !manifest
            .lines()
            .any(|l| l.trim_start().starts_with("tauri")),
        "the engine crate must not depend on Tauri"
    );
}

#[test]
fn only_the_ffmpeg_installer_uses_the_network() {
    let installer = core_dir().join("handlers").join("video").join("install.rs");
    let offenders: Vec<_> = rust_files(&core_dir())
        .into_iter()
        .filter(|p| *p != installer && !p.ends_with("architecture_tests.rs"))
        .filter(|p| code(p).contains("ureq"))
        .collect();
    assert!(
        offenders.is_empty(),
        "network client used outside the installer: {offenders:?}"
    );
}
