//! Truncated and odd .docx files must never panic: `docx::import` returns
//! a book (with warnings) or an `ImportError`, nothing else.
//!
//! Every golden fixture is truncated as a whole, and each XML part inside it is
//! mutated with a seeded generator and re-zipped.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// xorshift64*: small, seeded, the same on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Fragments that stress the XML layer: unbalanced and stray tags,
/// entities, CDATA, a BOM, control characters, attributes without values,
/// odd namespaces, and huge numbers where Word puts sizes and ids.
const ODD: &[&str] = &[
    "<w:p>",
    "</w:p>",
    "<w:r><w:t>",
    "</w:t></w:r>",
    "<w:br/>",
    "<w:tab/>",
    "<w:footnoteReference w:id=\"99999999999999999999\"/>",
    "<w:sz w:val=\"-1\"/>",
    "<w:sz w:val=\"4294967296\"/>",
    "<w:pStyle w:val=\"\"/>",
    "<w:ind w:firstLine=\"NaN\"/>",
    "<w:numPr><w:ilvl w:val=\"999\"/><w:numId w:val=\"-3\"/></w:numPr>",
    "<w:sym w:font=\"Wingdings\" w:char=\"ZZZZ\"/>",
    "<w:drawing><a:blip r:embed=\"rIdMissing\"/></w:drawing>",
    "<w:fldChar w:fldCharType=\"begin\"/>",
    "<w:instrText>HYPERLINK \"</w:instrText>",
    "<w:sectPr><w:type w:val=\"nextPage\"/></w:sectPr>",
    "<w:ins><w:del>",
    "<x:unknown xmlns:x=\"urn:x\" x:a=\"&#0;\">",
    "&amp;",
    "&bogus;",
    "&#xFFFFFFFF;",
    "&#55296;",
    "<![CDATA[",
    "]]>",
    "<!--",
    "-->",
    "<?pi ?>",
    "<!DOCTYPE w [<!ENTITY a \"aaaaaaaaaa\">]>",
    "\u{FEFF}",
    "\u{0}",
    "\u{1F}",
    "\"",
    "<",
    ">",
    "=",
];

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docx");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    paths
}

fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().unwrap().into_owned();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}

fn rezip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        zip.start_file(name.as_str(), SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn mutate(rng: &mut Rng, data: &mut Vec<u8>) -> String {
    let len = data.len();
    let at = rng.below(len + 1);
    match rng.below(6) {
        0 => {
            data.truncate(at);
            format!("truncate at {at}")
        }
        1 => {
            let n = 1 + rng.below(4);
            for _ in 0..n {
                let i = rng.below(len);
                if let Some(b) = data.get_mut(i) {
                    *b ^= 1 << rng.below(8);
                }
            }
            format!("flip {n} bits")
        }
        2 => {
            let end = (at + 1 + rng.below(200)).min(len);
            data.drain(at..end);
            format!("delete {at}..{end}")
        }
        3 => {
            let end = (at + 1 + rng.below(200)).min(len);
            let copy = data[at..end].to_vec();
            let to = rng.below(len + 1);
            data.splice(to..to, copy);
            format!("copy {at}..{end} to {to}")
        }
        4 => {
            let odd = ODD[rng.below(ODD.len())];
            data.splice(at..at, odd.bytes());
            format!("insert {odd:?} at {at}")
        }
        _ => {
            // Swap a tag name for another one (structure stays balanced
            // often enough to reach the deeper layers).
            let names: [&[u8]; 8] = [
                b"w:p",
                b"w:r",
                b"w:t",
                b"w:pPr",
                b"w:rPr",
                b"w:tbl",
                b"w:tc",
                b"w:hyperlink",
            ];
            let from = names[rng.below(names.len())];
            let to = names[rng.below(names.len())];
            let mut i = at;
            while i + from.len() < data.len() && &data[i..i + from.len()] != from {
                i += 1;
            }
            if i + from.len() < data.len() {
                data.splice(i..i + from.len(), to.iter().copied());
            }
            format!("rename {} to {} near {at}", esc(from), esc(to))
        }
    }
}

fn esc(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Import and report a panic as a failure description.
fn check(bytes: &[u8], what: impl FnOnce() -> String, failures: &mut Vec<String>) {
    if catch_unwind(AssertUnwindSafe(|| setzer_import::docx::import(bytes))).is_err() {
        failures.push(what());
    }
}

#[test]
fn truncated_files_never_panic() {
    let mut failures = Vec::new();
    {
        for path in fixtures() {
            let bytes = fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            // Dense near both ends (headers, central directory), sparse
            // in between.
            let n = bytes.len();
            let cuts = (0..n.min(64))
                .chain((n.saturating_sub(256)..n).step_by(3))
                .chain((0..n).step_by((n / 97).max(1)));
            for cut in cuts {
                check(
                    &bytes[..cut],
                    || format!("{name}: truncated to {cut} bytes"),
                    &mut failures,
                );
            }
        }
    }
    assert!(failures.is_empty(), "panics:\n{}", failures.join("\n"));
}

#[test]
fn mutated_xml_parts_never_panic() {
    // The cost of `cargo test -p setzer-import` is almost all here: keep
    // XML parts × rounds near 2000 as the corpus grows (16 files, 146
    // parts on 2026-10-21).
    const ROUNDS_PER_PART: usize = 14;
    let mut failures = Vec::new();
    let mut rng = Rng(0x5E72_E12D_0C0F_FEE5);
    {
        for path in fixtures() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let entries = unzip(&fs::read(&path).unwrap());
            let xml_parts: Vec<usize> = (0..entries.len())
                .filter(|&i| entries[i].0.ends_with(".xml") || entries[i].0.ends_with(".rels"))
                .collect();
            for &part in &xml_parts {
                for round in 0..ROUNDS_PER_PART {
                    let mut entries = entries.clone();
                    let mut steps = Vec::new();
                    // One to three mutations stacked on the same part.
                    for _ in 0..1 + rng.below(3) {
                        steps.push(mutate(&mut rng, &mut entries[part].1));
                    }
                    let bytes = rezip(&entries);
                    check(
                        &bytes,
                        || {
                            format!(
                                "{name} {} round {round}: {}",
                                entries[part].0,
                                steps.join(", ")
                            )
                        },
                        &mut failures,
                    );
                }
            }
            // Parts missing altogether.
            for &part in &xml_parts {
                let mut entries = entries.clone();
                let gone = entries.remove(part).0;
                check(
                    &rezip(&entries),
                    || format!("{name} without {gone}"),
                    &mut failures,
                );
            }
        }
    }
    assert!(failures.is_empty(), "panics:\n{}", failures.join("\n"));
}

#[test]
fn degenerate_documents_never_panic() {
    let doc = |body: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        )
    };
    let deep = "<w:p>".repeat(10_000);
    let many = "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(20_000);
    let long_run = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", "a".repeat(1 << 20));
    let breaks = format!(
        "<w:p><w:r>{}</w:r></w:p>",
        "<w:br w:type=\"page\"/>".repeat(5_000)
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty document.xml", Vec::new()),
        ("not xml", b"\xFF\xFE\x00garbage".to_vec()),
        ("utf-16 bom only", vec![0xFF, 0xFE]),
        ("deep nesting", doc(&deep).into_bytes()),
        ("20k paragraphs", doc(&many).into_bytes()),
        ("1 MB run", doc(&long_run).into_bytes()),
        ("5k page breaks", doc(&breaks).into_bytes()),
        ("no body", br#"<w:document xmlns:w="x"/>"#.to_vec()),
    ];
    let mut failures = Vec::new();
    {
        for (what, xml) in cases {
            let bytes = rezip(&[("word/document.xml".into(), xml)]);
            check(&bytes, || what.to_string(), &mut failures);
        }
    }
    assert!(failures.is_empty(), "panics:\n{}", failures.join("\n"));
}
