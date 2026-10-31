//! Golden tests: every manuscript is imported and compared with its `.json`
//! (book, asset summaries, warnings).
//!
//! - `tests/fixtures/docx/*.docx` → the `.json` next to it.
//! - `tests/fixtures/md/*.md` → the `.json` next to it; image paths are
//!   relative to the manuscript.
//! - `samples/*.md` (the repository's samples, read in place so they cannot
//!   drift) → `tests/fixtures/md/samples/*.json`.
//!
//! After a deliberate change, rewrite the expectations with
//! `SETZER_BLESS=1 cargo test -p setzer-import --test golden` and review
//! the diff. A new fixture without a `.json` fails until it is blessed.

use serde_json::{Value, json};
use setzer_import::{ImportError, Imported};
use std::fs;
use std::path::{Path, PathBuf};

fn snapshot(imported: Result<Imported, ImportError>) -> Value {
    let im = match imported {
        Ok(im) => im,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let assets: Vec<Value> = im
        .assets
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "media_type": a.media_type,
                "bytes": a.bytes.len(),
                "width_px": a.width_px,
                "height_px": a.height_px,
                "dpi": a.dpi,
            })
        })
        .collect();
    json!({ "book": im.book, "assets": assets, "warnings": im.warnings })
}

fn docx(path: &Path) -> Value {
    snapshot(setzer_import::docx::import(&fs::read(path).unwrap()))
}

fn markdown(path: &Path) -> Value {
    let src = fs::read_to_string(path).unwrap();
    let dir = path.parent().unwrap();
    snapshot(setzer_import::markdown::import(&src, |p| {
        fs::read(dir.join(p)).ok()
    }))
}

fn fixture_dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

/// Files in `dir` with extension `ext`, sorted, at least one.
fn sources(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no .{ext} files in {}", dir.display());
    paths
}

/// Compare (or, with `SETZER_BLESS`, write) each `(source, expected)` pair.
fn check(cases: &[(PathBuf, PathBuf)], snapshot: fn(&Path) -> Value) {
    let bless = std::env::var_os("SETZER_BLESS").is_some();
    let mut failed = Vec::new();
    for (path, expected_path) in cases {
        let got = serde_json::to_string_pretty(&snapshot(path)).unwrap() + "\n";
        if bless {
            fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
            fs::write(expected_path, &got).unwrap();
            continue;
        }
        match fs::read_to_string(expected_path) {
            Ok(expected) if expected == got => {}
            Ok(_) => failed.push(format!("{}: output differs", path.display())),
            Err(_) => failed.push(format!("{}: no .json (bless it)", path.display())),
        }
    }
    assert!(
        failed.is_empty(),
        "{}\nRun with SETZER_BLESS=1 to accept the new output, then review the diff.",
        failed.join("\n")
    );
}

fn next_to(paths: Vec<PathBuf>) -> Vec<(PathBuf, PathBuf)> {
    paths
        .into_iter()
        .map(|p| {
            let json = p.with_extension("json");
            (p, json)
        })
        .collect()
}

#[test]
fn docx_fixtures() {
    check(&next_to(sources(&fixture_dir("docx"), "docx")), docx);
}

#[test]
fn markdown_samples() {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let out = fixture_dir("md/samples");
    let cases: Vec<_> = sources(&samples, "md")
        .into_iter()
        .filter(|p| p.file_stem().is_some_and(|s| s != "README"))
        .map(|p| {
            let json = out.join(p.with_extension("json").file_name().unwrap());
            (p, json)
        })
        .collect();
    assert_eq!(cases.len(), 2, "expected the EN and DE samples");
    check(&cases, markdown);
}
