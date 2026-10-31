//! Manuscript importers. Every importer returns a `Book` plus warnings and
//! never fails on odd input.
//! Only input that is not a manuscript at all (empty, not a .docx) is an
//! error.

mod assets;
pub mod docx;
mod file;
mod heading;
mod lang;
pub mod markdown;
pub mod scrub;
mod vocab;

pub use assets::{MAX_IMAGE_BYTES, unique_id};
pub use file::{ReadError, read_image, read_manuscript};

use serde::Serialize;

/// A non-fatal problem found while importing, shown to the author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportWarning {
    pub kind: WarningKind,
    /// 1-based line in the source (Markdown) or paragraph number (docx);
    /// 0 when the warning is about the file as a whole.
    pub location: usize,
    /// Index into `Book::sections` of the section the warning is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<usize>,
    pub message: String,
    /// The thing the warning is about, when it has a name, so the UI can
    /// say which image or footnote it means.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<WarningDetail>,
}

/// What a warning names. Tagged by `kind` with kebab-case values, fields in
/// snake_case, like the façade's other enums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WarningDetail {
    /// An image that is missing, cannot be read or is in a format books
    /// cannot use: its path in the manuscript (Markdown) or in the file
    /// (.docx), or none when the file does not say.
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A footnote label (Markdown).
    Footnote { label: String },
    /// A front-matter key (Markdown YAML).
    FrontMatterKey { key: String },
    /// The name of a `:::` fence that was not closed (Markdown).
    Fence { name: String },
}

/// What a warning is about, so the UI can group and link warnings and
/// tests can assert on them without matching prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningKind {
    NoTitle,
    NoChapters,
    TextBeforeFirstChapter,
    TitlePageReplaced,
    TableOfContentsRemoved,
    /// A table that stayed paragraphs, in reading order: a one-column table
    /// (a boxed note), a table with a picture in a cell, a table with a
    /// single cell. Every other table becomes a table block.
    TableFlattened,
    /// A table became a table block, or was dropped because it was empty,
    /// and something in it could not be kept: a nested table, rotated text,
    /// a list or several paragraphs in a cell, a cell past the header's
    /// count (Markdown), a merge Word wrote inconsistently (.docx), a table
    /// with cells but no rows or no text (dropped). Formatting a theme
    /// decides anyway (shading, borders, fonts) gives no warning. One per
    /// table.
    TableSimplified,
    TextBoxSkipped,
    ImageUnsupported,
    ImageMissing,
    TrackedChangesAccepted,
    LineBreaksJoined,
    PageBreaksIgnored,
    SceneBreaksFromBlankLines,
    DamagedFile,
    /// A footnote mark without a definition (Markdown).
    FootnoteMissing,
    /// A footnote definition no mark uses, or a second one for a label
    /// (Markdown); its text is not in the book.
    FootnoteUnused,
    /// A `:::verse` fence without its closing `:::` (Markdown).
    VerseNotClosed,
    /// A front-matter key Kernsetzer does not read, or a value or line it
    /// could not read (Markdown YAML); left out of the book.
    FrontMatterKeyIgnored,
    /// A numbered list that does not start at 1, kept as paragraphs with
    /// their numbers (Markdown; the model numbers lists from 1).
    ListFlattened,
    /// A closing "END" / "Ende" under the manuscript's last paragraph,
    /// left out of the book (.docx).
    EndMarkRemoved,
    /// A picture in front of or behind the text (.docx, `wp:wrapNone`): it
    /// is set in the text where it was anchored, its position is not kept.
    /// One per book.
    ImagePositionDropped,
}

/// The attribution in an epigraph line that starts with a dash (`—`, `–`,
/// `--`, `-`), or `None`. Shared by the Markdown and .docx importers.
pub(crate) fn attribution_text(line: &str) -> Option<&str> {
    let text = line
        .strip_prefix(['—', '―', '–', '-'])?
        .trim_start_matches(['—', '―', '–', '-'])
        .trim();
    (!text.is_empty()).then_some(text)
}

/// Whether a front-matter paragraph starts like a dedication ("For …",
/// "Für …", "In memory of …", "Gewidmet …"). Shared by the Markdown and
/// .docx importers; each adds its own limits on length.
pub(crate) fn is_dedication(text: &str) -> bool {
    let l = text.to_lowercase();
    vocab::DEDICATION_STARTS.iter().any(|p| l.starts_with(p))
}

/// An image file found in a manuscript, referenced by `Block::Image::asset`.
pub use setzer_model::asset::Asset;

#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub book: setzer_model::Book,
    pub assets: Vec<Asset>,
    pub warnings: Vec<ImportWarning>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the file is empty")]
    Empty,
    #[error("this is not a Word document (.docx): {0}")]
    NotDocx(String),
}
