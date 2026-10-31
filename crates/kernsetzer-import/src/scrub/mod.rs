//! The scrambled copy of a manuscript.
//!
//! An author whose import fails can send a copy of the manuscript with every
//! letter replaced and the structure kept: most import bugs survive, none of
//! the prose does. `scrub` makes the copy, `check` imports the original and
//! the copy and says whether the copy still shows the problem.
//!
//! What a copy keeps and changes:
//!
//! - **Letters** change, one for one: a vowel for a vowel, a consonant for a
//!   consonant, an umlaut for an umlaut, ß for ß, upper case for upper case.
//!   A word keeps its length, and is replaced the same way wherever it
//!   stands in the file (so a title page and the document properties still
//!   agree, and a contents entry still matches its heading).
//! - **Everything else** stays: digits, punctuation, quotation marks,
//!   dashes, spaces, no-break and zero-width characters, soft hyphens,
//!   combining accents, line breaks, markup.
//! - **The words the importer reads** stay, because it decides by them:
//!   its heading, section and number words, "END"/"Ende", months, "by"/
//!   "von", the starts of dedications and copyright lines, and the function
//!   words it counts to tell English from German (`vocab`, `lang`: taken
//!   from the importer itself). Heading-type words stay only in short lines
//!   and only where the importer reads them; in running text they are
//!   replaced like any other word.
//! - **A .docx** is rewritten entry by entry: text in `document.xml`, notes,
//!   headers, footers, comments, text boxes and the document properties is
//!   replaced in place; every picture becomes a blank one of the same
//!   format, pixel size, resolution, colour mode and rotation tag
//!   (`image`); every other entry is copied byte for byte.
//! - **Markdown** keeps its front-matter keys and language, link and image
//!   targets, footnote labels, fences and attributes.
//!
//! The replacement is deterministic: its seed is a hash of the file, so the
//! same file always gives the same copy (tests are stable) and two files
//! give two different mappings. It is not a cipher: the original letters
//! are not in the copy, only a random choice of the same kind, so nothing
//! can be undone from the copy alone; and without the original there is no
//! seed to find, only a hash of text nobody else has.
//!
//! A file the scrubber cannot read is an error, never a repaired copy.

mod check;
mod docx;
mod image;
mod markdown;
mod words;
mod xml;

use std::path::Path;

pub use check::{Verdict, check};
pub use markdown::ScrubbedMarkdown;

/// Why a scrambled copy could not be made. The app then offers the
/// original.
#[derive(Debug, thiserror::Error)]
pub enum ScrubError {
    #[error("this is not a Word document (.docx): {0}")]
    NotDocx(String),
    #[error("not UTF-8 text")]
    NotUtf8,
    /// A part of the file that is damaged or in a form the scrubber does not
    /// handle: it would have to guess, and a guess is not a copy.
    #[error("{part} cannot be scrambled: {why}")]
    Part { part: String, why: String },
    #[error("{0} is too large")]
    TooLarge(String),
    /// A picture that cannot be blanked (damaged, or too large).
    #[error("the picture {name} cannot be blanked: {why}")]
    Picture { name: String, why: String },
}

/// What kind of manuscript a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Markdown,
}

impl Kind {
    /// By file extension (`.docx`; `.md`, `.markdown`), as `read_manuscript`
    /// does; `None` for any other file.
    pub fn of_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase()
            .as_str()
        {
            "docx" => Some(Kind::Docx),
            "md" | "markdown" => Some(Kind::Markdown),
            _ => None,
        }
    }
}

/// A scrambled manuscript: the file's bytes, and for Markdown the blank
/// pictures it refers to (see [`ScrubbedMarkdown::images`]); a .docx holds
/// its own.
#[derive(Debug, Clone, PartialEq)]
pub struct Scrubbed {
    pub bytes: Vec<u8>,
    pub images: Vec<(String, Vec<u8>)>,
}

/// The scrambled copy of a manuscript's bytes. `images` gives the bytes of
/// a picture a Markdown manuscript refers to, by the path written in it
/// (as for `markdown::import`); a .docx does not use it.
pub fn scrub(
    kind: Kind,
    bytes: &[u8],
    images: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Scrubbed, ScrubError> {
    match kind {
        Kind::Docx => Ok(Scrubbed {
            bytes: scrub_docx(bytes)?,
            images: Vec::new(),
        }),
        Kind::Markdown => {
            let md = scrub_markdown(bytes, images)?;
            Ok(Scrubbed {
                bytes: md.text.into_bytes(),
                images: md.images,
            })
        }
    }
}

/// The scrambled copy of a .docx.
pub fn scrub_docx(bytes: &[u8]) -> Result<Vec<u8>, ScrubError> {
    docx::scrub(bytes)
}

/// The scrambled copy of a Markdown manuscript, with a blank picture for
/// each one the original's import found.
pub fn scrub_markdown(
    bytes: &[u8],
    images: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<ScrubbedMarkdown, ScrubError> {
    markdown::scrub(bytes, images)
}
