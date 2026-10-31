//! Does the copy show the problem? Import the original and the copy and
//! compare what the author saw go wrong: the error, the warnings, and the
//! shape of the book (its language, and the sequence of section and block
//! kinds).
//!
//! Warning messages quote the manuscript ("the words after the first
//! break"), so they differ in a copy by design; a warning is compared by
//! its kind, its location, its section and what it names (a picture, a
//! footnote label, a front-matter key, a fence).

use setzer_model::{Block, Book};

use super::{Kind, Scrubbed};
use crate::{ImportError, Imported, WarningDetail, WarningKind};

/// The result of [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether the copy imports like the original.
    pub shows_problem: bool,
    /// What differs, in words for the person who reads the report. Empty
    /// when `shows_problem`.
    pub differences: Vec<String>,
}

type Key = (WarningKind, usize, Option<usize>, Option<WarningDetail>);

/// What an import came to, minus the text.
struct Summary {
    error: Option<String>,
    warnings: Vec<Key>,
    shape: Vec<String>,
}

fn summarize(result: Result<Imported, ImportError>) -> Summary {
    match result {
        Err(e) => Summary {
            error: Some(e.to_string()),
            warnings: Vec::new(),
            shape: Vec::new(),
        },
        Ok(im) => Summary {
            error: None,
            warnings: im
                .warnings
                .into_iter()
                .map(|w| (w.kind, w.location, w.section, w.detail))
                .collect(),
            shape: shape(&im.book),
        },
    }
}

/// One line per section and per block: the kinds, nothing of the text.
fn shape(book: &Book) -> Vec<String> {
    // The language chooses quotes and hyphenation: a copy that reads as
    // another language does not show a quote bug.
    let mut out = vec![format!("language {}", book.metadata.lang.0)];
    for (i, s) in book.sections.iter().enumerate() {
        let epigraph = if s.epigraph.is_some() {
            " +epigraph"
        } else {
            ""
        };
        out.push(format!(
            "section {i}: {:?} {:?}{epigraph}",
            s.kind, s.numbering
        ));
        for (j, b) in s.blocks.iter().enumerate() {
            out.push(format!("section {i}, block {j}: {}", block_kind(b)));
        }
    }
    out
}

fn block_kind(b: &Block) -> String {
    match b {
        Block::Paragraph { .. } => "paragraph".into(),
        Block::Heading { level, .. } => format!("heading {level}"),
        Block::SceneBreak { .. } => "scene break".into(),
        Block::BlockQuote { blocks, .. } => {
            let kinds: Vec<String> = blocks.iter().map(block_kind).collect();
            format!("quote [{}]", kinds.join(", "))
        }
        Block::Verse { lines, .. } => format!("verse of {} lines", lines.len()),
        // A picture's place, side and width are what the importer reads from
        // a .docx's `wp:anchor` (the copy must not change them).
        Block::Image {
            placement, side, ..
        } => {
            let width = b
                .image_width()
                .map_or("auto".to_string(), |w| format!("{w}%"));
            format!(
                "image {} side {} width {width}",
                format!("{placement:?}").to_lowercase(),
                format!("{side:?}").to_lowercase()
            )
        }
        Block::List { ordered, items, .. } => {
            let kind = if *ordered { "numbered" } else { "bulleted" };
            format!("{kind} list of {} items", items.len())
        }
        Block::Table(table) => {
            let l = table.layout();
            format!(
                "table of {} rows, {} columns, {} header rows",
                l.rows, l.columns, l.header_rows
            )
        }
        Block::PageBreak { .. } => "page break".into(),
    }
}

/// Import the original and the copy and compare. `original_images` gives
/// the original Markdown manuscript's pictures, as for `scrub`; the copy's
/// are its own.
pub fn check(
    kind: Kind,
    original: &[u8],
    original_images: impl FnMut(&str) -> Option<Vec<u8>>,
    copy: &Scrubbed,
) -> Verdict {
    let (a, b) = match kind {
        Kind::Docx => (
            summarize(crate::docx::import(original)),
            summarize(crate::docx::import(&copy.bytes)),
        ),
        Kind::Markdown => {
            let strip = |b: &[u8]| {
                std::str::from_utf8(b).map(|s| s.strip_prefix('\u{feff}').unwrap_or(s).to_string())
            };
            let (Ok(orig), Ok(copy_text)) = (strip(original), strip(&copy.bytes)) else {
                return Verdict {
                    shows_problem: false,
                    differences: vec!["the original or the copy is not UTF-8 text".into()],
                };
            };
            let copy_images = |p: &str| {
                copy.images
                    .iter()
                    .find(|(path, _)| path == p)
                    .map(|(_, bytes)| bytes.clone())
            };
            (
                summarize(crate::markdown::import(&orig, original_images)),
                summarize(crate::markdown::import(&copy_text, copy_images)),
            )
        }
    };
    compare(&a, &b)
}

fn compare(a: &Summary, b: &Summary) -> Verdict {
    let mut differences = Vec::new();
    if a.error != b.error {
        differences.push(format!(
            "the import's error differs: the original gives {}, the copy {}",
            describe(a.error.as_deref()),
            describe(b.error.as_deref())
        ));
    }
    if a.warnings != b.warnings {
        let only = |x: &[Key], y: &[Key]| -> Vec<String> {
            let mut rest: Vec<&Key> = y.iter().collect();
            x.iter()
                .filter(|k| match rest.iter().position(|r| r == k) {
                    Some(at) => {
                        rest.remove(at);
                        false
                    }
                    None => true,
                })
                .map(|(kind, loc, _, detail)| match detail {
                    Some(d) => format!("{kind:?} at {loc} ({d:?})"),
                    None => format!("{kind:?} at {loc}"),
                })
                .collect()
        };
        let (lost, gained) = (
            only(&a.warnings, &b.warnings),
            only(&b.warnings, &a.warnings),
        );
        if lost.is_empty() && gained.is_empty() {
            differences.push("the warnings come in a different order".into());
        }
        if !lost.is_empty() {
            differences.push(format!("the copy lacks the warnings: {}", lost.join("; ")));
        }
        if !gained.is_empty() {
            differences.push(format!("the copy adds the warnings: {}", gained.join("; ")));
        }
    }
    if a.shape != b.shape {
        let at = a
            .shape
            .iter()
            .zip(&b.shape)
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.shape.len().min(b.shape.len()));
        let line = |s: &Summary| s.shape.get(at).cloned().unwrap_or_else(|| "nothing".into());
        differences.push(format!(
            "the book's shape differs from line {} on: the original has \"{}\", the copy \"{}\" \
             ({} and {} lines)",
            at + 1,
            line(a),
            line(b),
            a.shape.len(),
            b.shape.len()
        ));
    }
    Verdict {
        shows_problem: differences.is_empty(),
        differences,
    }
}

fn describe(error: Option<&str>) -> String {
    error.map_or_else(|| "none".into(), |e| format!("\"{e}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markdown(src: &str) -> Summary {
        summarize(crate::markdown::import(src, |_| None))
    }

    #[test]
    fn equal_shapes_and_warnings_show_the_problem() {
        // links.md / footnotes.md lines: a missing note warns.
        let a =
            markdown("Title: T\n\n# One\n\nA mark without a definition[^nowhere] stays as text.\n");
        let b =
            markdown("Title: T\n\n# One\n\nA mark without a definition[^nowhere] stays as text.\n");
        let v = compare(&a, &b);
        assert!(v.shows_problem && v.differences.is_empty());
    }

    #[test]
    fn a_lost_warning_a_new_block_and_another_error_are_reported() {
        let a =
            markdown("Title: T\n\n# One\n\nA mark without a definition[^nowhere] stays as text.\n");
        let b = markdown(
            "Title: T\n\n# One\n\nA mark without a definition stays as text.\n\n* * *\n\nMore.\n",
        );
        let v = compare(&a, &b);
        assert!(!v.shows_problem);
        let all = v.differences.join("\n");
        assert!(
            all.contains("lacks the warnings") && all.contains("FootnoteMissing"),
            "{all}"
        );
        assert!(all.contains("shape differs"), "{all}");
        let empty = summarize(crate::markdown::import("\n", |_| None));
        let v = compare(&a, &empty);
        assert!(v.differences.iter().any(|d| d.contains("error differs")));
    }
}
