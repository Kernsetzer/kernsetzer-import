//! The .docx container: zip entries, relationships and size limits.

use crate::ImportError;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Largest XML part we read. A 300k-word novel's `document.xml` is ~15 MB;
/// anything far beyond that is a zip bomb, not a manuscript.
pub(super) const MAX_XML_BYTES: u64 = 200 * 1024 * 1024;
/// Largest image we import.
pub(super) const MAX_MEDIA_BYTES: u64 = crate::MAX_IMAGE_BYTES;

pub(super) struct Package<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    /// Lowercased entry name → actual entry name (OPC names are
    /// case-insensitive, producers are not always consistent).
    names: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rel {
    pub kind: RelKind,
    /// Zip path for internal targets, the URL for external ones.
    pub target: String,
    pub external: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RelKind {
    OfficeDocument,
    CoreProperties,
    ExtendedProperties,
    Styles,
    Numbering,
    Footnotes,
    Endnotes,
    Image,
    Hyperlink,
    Other,
}

impl RelKind {
    fn from_type(t: &str) -> Self {
        // Transitional and strict OOXML use different URIs with the same
        // last segment.
        match t.rsplit('/').next().unwrap_or_default() {
            "officeDocument" => RelKind::OfficeDocument,
            "core-properties" => RelKind::CoreProperties,
            "extended-properties" => RelKind::ExtendedProperties,
            "styles" => RelKind::Styles,
            "numbering" => RelKind::Numbering,
            "footnotes" => RelKind::Footnotes,
            "endnotes" => RelKind::Endnotes,
            "image" => RelKind::Image,
            "hyperlink" => RelKind::Hyperlink,
            _ => RelKind::Other,
        }
    }
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, ImportError> {
        if bytes.is_empty() {
            return Err(ImportError::Empty);
        }
        let zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::NotDocx(format!("not a zip archive ({e})")))?;
        let names = zip
            .file_names()
            .filter_map(Result::ok)
            .map(|n| (n.to_lowercase(), n.into_owned()))
            .collect();
        Ok(Self { zip, names })
    }

    fn entry_name(&self, path: &str) -> Option<String> {
        self.names
            .get(&path.trim_start_matches('/').to_lowercase())
            .cloned()
    }

    pub fn has(&self, path: &str) -> bool {
        self.entry_name(path).is_some()
    }

    /// Raw bytes of an entry. `None` if missing, `Err` if unreadable or
    /// larger than `limit`.
    pub fn read_bytes(&mut self, path: &str, limit: u64) -> Option<Result<Vec<u8>, String>> {
        let name = self.entry_name(path)?;
        Some(self.read_entry(&name, limit))
    }

    fn read_entry(&mut self, name: &str, limit: u64) -> Result<Vec<u8>, String> {
        let file = self.zip.by_name(name).map_err(|e| e.to_string())?;
        if file.size() > limit {
            return Err(format!("{name} is too large ({} bytes)", file.size()));
        }
        let mut buf = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        // `take` guards against a size field that lies.
        file.take(limit + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() as u64 > limit {
            return Err(format!("{name} is too large"));
        }
        Ok(buf)
    }

    /// An XML part as text. Handles UTF-8 (with or without BOM) and UTF-16.
    pub fn read_xml(&mut self, path: &str) -> Option<Result<String, String>> {
        Some(
            self.read_bytes(path, MAX_XML_BYTES)?
                .map(|b| decode_text(&b)),
        )
    }

    /// Relationships of a part, keyed by relationship id. Targets are
    /// resolved to zip paths relative to the part.
    pub fn rels(&mut self, part: &str) -> HashMap<String, Rel> {
        let (dir, file) = match part.rsplit_once('/') {
            Some((d, f)) => (d, f),
            None => ("", part),
        };
        let rels_path = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let Some(Ok(xml)) = self.read_xml(&rels_path) else {
            return HashMap::new();
        };
        parse_rels(&xml, dir)
    }
}

fn parse_rels(xml: &str, base_dir: &str) -> HashMap<String, Rel> {
    use quick_xml::events::Event;
    let mut out = HashMap::new();
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e))
                if e.local_name().into_inner() == "Relationship" =>
            {
                let get = |k| super::xml::attr(&e, k);
                let (Some(id), Some(target)) = (get("Id"), get("Target")) else {
                    continue;
                };
                let external = get("TargetMode").is_some_and(|m| m == "External");
                let kind = RelKind::from_type(&get("Type").unwrap_or_default());
                let target = if external {
                    target
                } else {
                    resolve_target(base_dir, &target)
                };
                out.insert(
                    id,
                    Rel {
                        kind,
                        target,
                        external,
                    },
                );
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Resolve a relationship target against the source part's directory:
/// `media/a.png` from `word` → `word/media/a.png`; absolute targets
/// (`/word/media/a.png`) are package-relative; `..` is collapsed.
pub(super) fn resolve_target(base_dir: &str, target: &str) -> String {
    let target = percent_decode(target);
    let mut parts: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        parts.extend(base_dir.split('/').filter(|s| !s.is_empty()));
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = bytes.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(h).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode an XML part: UTF-16 by BOM, otherwise UTF-8 (lossy, BOM dropped).
pub(super) fn decode_text(b: &[u8]) -> String {
    let utf16 = |le: bool| {
        let units: Vec<u16> = b[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match b {
        [0xFF, 0xFE, ..] => utf16(true),
        [0xFE, 0xFF, ..] => utf16(false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(b).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_relative_to_part() {
        assert_eq!(
            resolve_target("word", "media/image1.png"),
            "word/media/image1.png"
        );
        assert_eq!(
            resolve_target("word", "/word/media/a.png"),
            "word/media/a.png"
        );
        assert_eq!(
            resolve_target("word", "../media/a%20b.png"),
            "media/a b.png"
        );
        assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
    }

    #[test]
    fn rels_are_parsed() {
        let xml = r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
  <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/?a=1&amp;b=2" TargetMode="External"/>
</Relationships>"#;
        let rels = parse_rels(xml, "word");
        assert_eq!(rels["rId1"].kind, RelKind::Styles);
        assert_eq!(rels["rId5"].target, "word/media/image1.png");
        assert!(rels["rId9"].external);
        assert_eq!(rels["rId9"].target, "https://example.com/?a=1&b=2");
    }

    #[test]
    fn decodes_bom_and_utf16() {
        assert_eq!(decode_text(b"\xEF\xBB\xBF<a/>"), "<a/>");
        assert_eq!(decode_text(b"\xFF\xFE<\x00a\x00/\x00>\x00"), "<a/>");
    }
}
