//! Build script: inject git sha + commit date into the binary so
//! `runcomfy --version` can show exactly which commit shipped.
//!
//! Reads the local `.git` directory at build time. No network access,
//! and the repository can be public or private — the resulting short
//! sha leaks nothing about the code.
//!
//! In environments without a `.git` directory (cargo install from a
//! crate, Docker build from a tarball, GitHub Actions checkout with
//! `fetch-depth: 0` missing, etc.), both values fall back to
//! `"unknown"` and the build still succeeds.
//!
//! Honors `RUNCOMFY_GIT_SHA` / `RUNCOMFY_BUILD_DATE` env vars if the
//! caller wants to override (CI release builds usually do).

use std::process::Command;

fn main() {
    // Re-run the build script if HEAD or any tracked ref moves.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    println!("cargo:rerun-if-env-changed=RUNCOMFY_GIT_SHA");
    println!("cargo:rerun-if-env-changed=RUNCOMFY_BUILD_DATE");

    let sha = std::env::var("RUNCOMFY_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(detect_git_sha);
    let date = std::env::var("RUNCOMFY_BUILD_DATE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(detect_commit_date);

    println!("cargo:rustc-env=RUNCOMFY_GIT_SHA={}", sha);
    println!("cargo:rustc-env=RUNCOMFY_BUILD_DATE={}", date);
}

fn detect_git_sha() -> String {
    let short = run_git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
    if short == "unknown" {
        return short;
    }
    // Append `-dirty` if the working tree has uncommitted changes, so the
    // version unambiguously reflects whether this binary was cleanly built.
    let dirty = run_git(&["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if dirty {
        format!("{short}-dirty")
    } else {
        short
    }
}

fn detect_commit_date() -> String {
    // ISO 8601 committer date of HEAD, e.g. `2026-04-28`.
    run_git(&["log", "-1", "--format=%cd", "--date=short", "HEAD"])
        .unwrap_or_else(|| "unknown".into())
}

fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
