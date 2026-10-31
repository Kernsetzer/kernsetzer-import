//! The scrambled copy of every .docx and Markdown fixture
//! of the golden corpus: it imports to the same warnings and the same
//! sequence of block kinds as the original, and the prose is gone.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use setzer_import::scrub::{Kind, ScrubError, check, scrub, scrub_docx};

fn fixture_dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
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

fn md_sources() -> Vec<PathBuf> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let mut all = Vec::new();
    all.extend(
        files(&samples, "md")
            .into_iter()
            .filter(|p| p.file_stem().is_some_and(|s| s != "README")),
    );
    all
}

/// Words of seven letters or more, lowercased: long enough that a copy
/// keeping one would be keeping prose.
fn long_words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 7)
        .map(str::to_lowercase)
        .collect()
}

/// The text of a .docx's `w:t` elements (a rough scan, independent of the
/// importer and of the scrubber).
fn docx_text(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    for name in [
        "word/document.xml",
        "word/footnotes.xml",
        "word/endnotes.xml",
    ] {
        if let Ok(mut f) = zip.by_name(name) {
            let mut part = String::new();
            f.read_to_string(&mut part).unwrap();
            xml.push_str(&part);
        }
    }
    let mut out = String::new();
    let mut rest = xml.as_str();
    while let Some(at) = rest.find("<w:t") {
        rest = &rest[at + 4..];
        let Some(open_end) = rest.find('>') else {
            break;
        };
        if rest.starts_with(['>', ' ']) && !rest[..open_end].ends_with('/') {
            let body = &rest[open_end + 1..];
            let end = body.find("</w:t>").unwrap_or(body.len());
            out.push_str(&body[..end]);
            out.push(' ');
        }
    }
    out
}

/// Words that are the importer's or the format's, so a copy keeps them.
const MAY_STAY: &[&str] = &[
    // headings, sections, closing marks, copyright pages, datelines
    "chapter",
    "kapitel",
    "prologue",
    "epilogue",
    "contents",
    "copyright",
    "acknowledgements",
    "acknowledgments",
    "afterword",
    "foreword",
    "preface",
    "nachwort",
    "vorwort",
    "danksagung",
    "inhalt",
    "dedication",
    "widmung",
    "impressum",
    "reserved",
    "vorbehalten",
    "autorin",
    "september",
    "november",
    "december",
    "february",
    "october",
    "oktober",
    "dezember",
    "februar",
    "january",
    "august",
    "weitere",
    // front-matter keys and Markdown names
    "subtitle",
    "language",
    "publisher",
    "author",
    "verse",
    "epigraph",
    "smallcaps",
    "unnumbered",
    "underline",
    "keywords",
    "abstract",
    "newpage",
    "pagebreak",
    // number words and the placeholder the importer reads
    "siebzehn",
    "zweites",
    "zweiter",
    "unbekannt",
    // footnote labels
    "nowhere",
    "lanterns",
    "telegram",
    // the function words that tell English from German are short; these
    // long words come from the text of fixtures that name their own file
    // types or addresses
    "example",
    "mailto",
    "calloway",
    "karte",
    "https",
];

#[test]
fn every_docx_fixture_imports_the_same_after_scrambling() {
    let mut bad = Vec::new();
    for path in files(&fixture_dir("docx"), "docx") {
        let original = fs::read(&path).unwrap();
        let copy = match scrub(Kind::Docx, &original, |_| None) {
            Ok(c) => c,
            Err(e) => {
                bad.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let verdict = check(Kind::Docx, &original, |_| None, &copy);
        if !verdict.shows_problem {
            bad.push(format!("{}: {:?}", path.display(), verdict.differences));
        }
        // The same book in the same language.
        let a = setzer_import::docx::import(&original);
        let b = setzer_import::docx::import(&copy.bytes);
        if let (Ok(a), Ok(b)) = (a, b) {
            assert_eq!(
                a.book.metadata.lang,
                b.book.metadata.lang,
                "{}",
                path.display()
            );
            assert_eq!(a.assets.len(), b.assets.len(), "{}", path.display());
            for (x, y) in a.assets.iter().zip(&b.assets) {
                assert_eq!(
                    (&x.media_type, x.width_px, x.height_px),
                    (&y.media_type, y.width_px, y.height_px),
                    "{}",
                    path.display()
                );
                assert_eq!(x.dpi.map(f32::round), y.dpi.map(f32::round));
            }
        }
        // The prose is gone.
        let survivors: Vec<String> = long_words(&docx_text(&original))
            .intersection(&long_words(&docx_text(&copy.bytes)))
            .filter(|w| !MAY_STAY.contains(&w.as_str()))
            .cloned()
            .collect();
        if !survivors.is_empty() {
            bad.push(format!("{}: words survive: {survivors:?}", path.display()));
        }
        // And the same input gives the same copy.
        assert_eq!(
            copy.bytes,
            scrub_docx(&original).unwrap(),
            "{}",
            path.display()
        );
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn every_markdown_fixture_imports_the_same_after_scrambling() {
    let mut bad = Vec::new();
    for path in md_sources() {
        let original = fs::read(&path).unwrap();
        let dir = path.parent().unwrap();
        let images = |p: &str| fs::read(dir.join(p)).ok();
        let copy = match scrub(Kind::Markdown, &original, images) {
            Ok(c) => c,
            Err(e) => {
                bad.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let verdict = check(Kind::Markdown, &original, images, &copy);
        if !verdict.shows_problem {
            bad.push(format!("{}: {:?}", path.display(), verdict.differences));
        }
        let text = String::from_utf8(original.clone()).unwrap();
        let copy_text = String::from_utf8(copy.bytes.clone()).unwrap();
        assert_eq!(text.lines().count(), copy_text.lines().count());
        let a = setzer_import::markdown::import(&text, images).unwrap();
        let b = setzer_import::markdown::import(&copy_text, |p| {
            copy.images
                .iter()
                .find(|(q, _)| q == p)
                .map(|(_, b)| b.clone())
        })
        .unwrap();
        assert_eq!(
            a.book.metadata.lang,
            b.book.metadata.lang,
            "{}",
            path.display()
        );
        let survivors: Vec<String> = long_words(&text)
            .intersection(&long_words(&copy_text))
            .filter(|w| !MAY_STAY.contains(&w.as_str()))
            .cloned()
            .collect();
        // Addresses and file names stay by design; the rest must not.
        let in_targets = |w: &String| {
            text.lines().any(|l| {
                l.to_lowercase().contains(w.as_str()) && (l.contains("](") || l.contains("<http"))
            })
        };
        let prose: Vec<_> = survivors.iter().filter(|w| !in_targets(w)).collect();
        if !prose.is_empty() {
            bad.push(format!("{}: words survive: {prose:?}", path.display()));
        }
        let again = scrub(Kind::Markdown, &original, images).unwrap();
        assert_eq!(copy, again, "{}", path.display());
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_file_the_scrubber_cannot_read_is_an_error() {
    assert!(matches!(
        scrub(Kind::Docx, b"not a zip", |_| None),
        Err(ScrubError::NotDocx(_))
    ));
    assert!(matches!(
        scrub(Kind::Docx, b"", |_| None),
        Err(ScrubError::NotDocx(_))
    ));
    assert!(matches!(
        scrub(Kind::Markdown, &[0xff, 0xfe, 0x80], |_| None),
        Err(ScrubError::NotUtf8)
    ));
    // A .docx whose main part is not well-formed XML is not repaired.
    let original = fs::read(fixture_dir("docx").join("synthetic-en-tables.docx")).unwrap();
    let broken = replace_entry(&original, "word/document.xml", |xml| {
        let mut xml = xml.to_vec();
        let at = xml.windows(6).position(|w| w == b"</w:p>").unwrap();
        xml.splice(at..at + 6, b"</w:r>".iter().copied());
        xml
    });
    assert!(matches!(
        scrub_docx(&broken),
        Err(ScrubError::Part { ref part, .. }) if part == "word/document.xml"
    ));
}

/// The zip with one entry's bytes changed.
fn replace_entry(zip_bytes: &[u8], name: &str, change: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    use std::io::Write;
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        let entry = f.name().unwrap().into_owned();
        if entry == name {
            data = change(&data);
        }
        out.start_file(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn damaged_files_never_panic_and_never_come_back_repaired() {
    let original = fs::read(fixture_dir("docx").join("synthetic-en-tables.docx")).unwrap();
    // Truncated zips.
    for n in (0..original.len()).step_by(original.len() / 40 + 1) {
        let _ = scrub_docx(&original[..n]);
    }
    // Bytes of the main part changed, deterministically: the scrubber
    // either errors or makes a copy that imports without a panic.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let (mut errors, mut copies) = (0, 0);
    for _ in 0..120 {
        let (at, byte) = (next() as usize, next() as u8);
        let damaged = replace_entry(&original, "word/document.xml", |xml| {
            let mut xml = xml.to_vec();
            let at = at % xml.len();
            xml[at] = byte;
            xml
        });
        match scrub_docx(&damaged) {
            Ok(copy) => {
                copies += 1;
                let _ = setzer_import::docx::import(&copy);
            }
            Err(_) => errors += 1,
        }
    }
    assert!(errors + copies == 120 && copies > 0);
    // Markdown cut anywhere.
    let md = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/de-sample.md"),
    )
    .unwrap();
    for n in (0..md.len()).step_by(997) {
        if md.is_char_boundary(n) {
            scrub(Kind::Markdown, &md.as_bytes()[..n], |_| None).unwrap();
        }
    }
}
