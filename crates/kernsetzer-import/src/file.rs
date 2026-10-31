//! Reading a manuscript file: the one place that turns a path into an
//! `Imported`, for the CLI and the app façade alike. The importers
//! themselves take bytes or text and never touch the filesystem.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use crate::{ImportError, Imported};

/// Why a manuscript file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Not a `.md`, `.markdown` or `.docx` file.
    #[error("not a Markdown or Word file")]
    Unsupported,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("not UTF-8 text")]
    NotUtf8,
    #[error("{0}")]
    Import(#[from] ImportError),
}

/// The book, its images and the import warnings, by file extension
/// (`.md`, `.markdown`, `.docx`). A Markdown manuscript's images are read
/// relative to its folder.
pub fn read_manuscript(path: &Path) -> Result<Imported, ReadError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("md" | "markdown") => {
            let bytes = fs::read(path)?;
            let src = std::str::from_utf8(&bytes).map_err(|_| ReadError::NotUtf8)?;
            // Windows editors start UTF-8 files with a byte order mark.
            let src = src.strip_prefix('\u{feff}').unwrap_or(src);
            let dir = path.parent().unwrap_or(Path::new(""));
            Ok(crate::markdown::import(src, |p| read_image(dir, p))?)
        }
        Some("docx") => Ok(crate::docx::import(&fs::read(path)?)?),
        _ => Err(ReadError::Unsupported),
    }
}

/// An image a Markdown manuscript refers to, relative to its folder. Reads
/// at most one byte over the import limit, so the importer can tell a file
/// that is too large without all of it in memory.
pub fn read_image(dir: &Path, path: &str) -> Option<Vec<u8>> {
    let file = dir.join(path);
    if !file.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(file)
        .ok()?
        .take(crate::MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}
