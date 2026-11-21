//! Records the git short hash of the source as `ZC_BUILD` ("unknown" when
//! git or the repository is not there, e.g. a source archive).
use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn main() {
    let build = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ZC_BUILD={build}");
    println!("cargo:rerun-if-changed=build.rs");
    // Rebuild when HEAD moves: HEAD itself, the branch it points at, packed refs.
    let paths = [
        git(&["rev-parse", "--git-path", "HEAD"]),
        git(&["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&["rev-parse", "--git-path", &r])),
        git(&["rev-parse", "--git-path", "packed-refs"]),
    ];
    for path in paths.into_iter().flatten().map(PathBuf::from) {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
