//! The scrambled copy of a .docx.
//!
//! The zip is rewritten entry by entry. An entry that holds prose or a
//! picture is replaced; every other entry (styles, settings, numbering,
//! fonts, relationships, content types, anything we do not know) is copied
//! compressed byte for byte (`raw_copy_file`), so page setup, fields,
//! style names and bookmark names are the original's.

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use super::words::{LANG_BUDGET, Scrambler, seed_of};
use super::xml::{self, Rules};
use super::{ScrubError, image};
use crate::ImportError;
use crate::docx::{Located, locate};

/// Largest XML part or picture we read (the importer's limits).
const MAX_XML_BYTES: u64 = 200 * 1024 * 1024;
const MAX_PICTURE_BYTES: u64 = crate::MAX_IMAGE_BYTES;

/// Prose in an XML part: runs of text (also deleted text, comments, notes,
/// headers, footers, text boxes, charts and diagrams), the names of the
/// people who changed or commented, and the descriptions of pictures,
/// tables and WordArt. Beyond the runs, any other text node of the part is
/// prose too (`customXml/` items, `docProps/custom.xml`), except the
/// elements of `KEEP`. Style names, bookmark names, field codes
/// (`instrText`), list templates and attributes the rules do not name
/// stay.
static WORD: Rules = Rules {
    text: &["t", "delText"],
    paragraph: &["p"],
    breaks: &[
        ("br", '\n'),
        ("cr", '\n'),
        ("tab", '\n'),
        ("ptab", '\n'),
        ("sym", '\n'),
        ("drawing", '\n'),
        ("pict", '\n'),
        ("object", '\n'),
        ("footnoteReference", '\n'),
        ("endnoteReference", '\n'),
        ("footnoteRef", '\n'),
        ("endnoteRef", '\n'),
        ("annotationRef", '\n'),
        ("commentReference", '\n'),
        ("fldChar", '\n'),
        ("instrText", '\n'),
        ("noBreakHyphen", '\u{2011}'),
        ("softHyphen", '\u{ad}'),
    ],
    attrs: &[
        ("*", "author"),
        ("*", "initials"),
        ("docPr", "descr"),
        ("docPr", "title"),
        ("cNvPr", "descr"),
        ("cNvPr", "title"),
        ("tblCaption", "val"),
        ("tblDescription", "val"),
        ("textpath", "string"),
    ],
    all_text: true,
    keep: KEEP,
};

/// Elements whose text is code, data or one of an enumerated set of
/// values, not prose: a field's instruction and data, the typed values of
/// custom properties, and the elements the importer or Word read as values:
///
/// - `align`: `wp:align` under `positionH`/`positionV` (`left`, `right`,
///   `center`, `inside`, `outside`, `top`, `bottom`), which the importer
///   reads for a picture's side.
/// - VML OLE objects: `LinkType`, `LockedField`, `FieldCodes`.
/// - Charts: `formatCode` (a number format) and `f` (a cell formula, also
///   `cx:f`).
/// - A bibliography source (`customXml/`): `SourceType` is enumerated,
///   `Tag` is what a `CITATION` field (kept) refers to, and `Guid` (hex, so
///   it has letters) and `LCID` (a language tag) are read by Word.
const KEEP: &[&str] = &[
    "instrText",
    "delInstrText",
    "fldData",
    "bool",
    "filetime",
    "date",
    "align",
    "LinkType",
    "LockedField",
    "FieldCodes",
    "formatCode",
    "f",
    "SourceType",
    "Tag",
    "Guid",
    "LCID",
];

/// `docProps/core.xml`: the document's title, author and the like (not its
/// language, nor its dates).
static CORE: Rules = Rules {
    text: &[
        "title",
        "creator",
        "subject",
        "description",
        "keywords",
        "lastModifiedBy",
        "category",
        "contentStatus",
    ],
    paragraph: &[],
    breaks: &[],
    attrs: &[],
    all_text: false,
    keep: &[],
};

/// `docProps/app.xml`: the company, the manager and the titles of parts.
static APP: Rules = Rules {
    text: &["Company", "Manager", "lpstr"],
    paragraph: &[],
    breaks: &[],
    attrs: &[],
    all_text: false,
    keep: &[],
};

/// What happens to a zip entry.
enum Entry {
    Copy,
    Text(&'static Rules),
    Picture,
}

/// An entry name as the importer compares it: no leading slash, no case.
fn key(name: &str) -> String {
    name.trim_start_matches('/').to_lowercase()
}

/// Formatting parts, by file name, wherever they sit: their text nodes
/// (there are none to speak of) and attributes (style names) stay.
const AS_IS: &[&str] = &[
    "[content_types].xml",
    "styles.xml",
    "stylesWithEffects.xml",
    "settings.xml",
    "fontTable.xml",
    "numbering.xml",
    "webSettings.xml",
];

/// Pictures by extension, in `media/` folders or not.
const PICTURES: &[&str] = &[
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "svg", "bmp", "tif", "tiff", "emf", "wmf",
    "emz", "wmz", "ico",
];

/// What happens to the zip entry `name`, given where the importer finds
/// the package's main part and properties. An XML part is scrambled by
/// what it is, not by its folder: unless it is formatting (`AS_IS`, a
/// theme, a relationships file), its text is prose.
fn classify(name: &str, at: &Located) -> Entry {
    let lower = key(name);
    if lower.ends_with('/') || lower.ends_with(".rels") {
        return Entry::Copy;
    }
    if lower == key(&at.main) {
        return Entry::Text(&WORD);
    }
    if lower == key(&at.core) {
        return Entry::Text(&CORE);
    }
    if lower == key(&at.app) {
        return Entry::Text(&APP);
    }
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    let ext = file.rsplit_once('.').map(|(_, e)| e);
    // Whatever Word keeps in a `media/` folder besides XML is a picture, in
    // a format named or not: `hdphoto1.wdp` (JPEG XR), HEIC, AVIF, PDF. The
    // blanker falls back to a header-only stand-in for formats it doesn't
    // know.
    if (lower.starts_with("media/") || lower.contains("/media/")) && ext != Some("xml") {
        return Entry::Picture;
    }
    if ext == Some("xml") {
        let as_is = AS_IS.iter().any(|a| file == a.to_lowercase())
            || lower.starts_with("theme/")
            || lower.contains("/theme/");
        return if as_is {
            Entry::Copy
        } else {
            Entry::Text(&WORD)
        };
    }
    match ext {
        Some(ext) if PICTURES.contains(&ext) => Entry::Picture,
        _ => Entry::Copy,
    }
}

pub(super) fn scrub(bytes: &[u8]) -> Result<Vec<u8>, ScrubError> {
    if bytes.is_empty() {
        return Err(ScrubError::NotDocx("the file is empty".into()));
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ScrubError::NotDocx(format!("not a zip archive ({e})")))?;
    // The importer's own question: which entry is the main document? A
    // package without one is no .docx (an .xlsx renamed, say), and the
    // copy of one is not offered.
    let at = locate(bytes).map_err(|e| match e {
        ImportError::NotDocx(why) => ScrubError::NotDocx(why),
        other => ScrubError::NotDocx(other.to_string()),
    })?;
    let mut names = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let file = zip.by_index_raw(i).map_err(|e| entry_error(i, &e))?;
        names.push(file.name().map_err(|e| entry_error(i, &e))?.into_owned());
    }
    if !names.iter().any(|n| key(n) == key(&at.main)) {
        return Err(ScrubError::NotDocx("it has no main document part".into()));
    }

    let sc = Scrambler::new(seed_of(bytes));
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (i, name) in names.iter().enumerate() {
        let part_error = |why: String| ScrubError::Part {
            part: name.clone(),
            why,
        };
        let entry = classify(name, &at);
        if matches!(entry, Entry::Copy) {
            let file = zip.by_index_raw(i).map_err(|e| part_error(e.to_string()))?;
            out.raw_copy_file(file)
                .map_err(|e| part_error(e.to_string()))?;
            continue;
        }
        let mut file = zip.by_index(i).map_err(|e| part_error(e.to_string()))?;
        let limit = if matches!(entry, Entry::Picture) {
            MAX_PICTURE_BYTES
        } else {
            MAX_XML_BYTES
        };
        if file.size() > limit {
            return Err(ScrubError::TooLarge(name.clone()));
        }
        let options = SimpleFileOptions::default()
            .compression_method(file.compression())
            .last_modified_time(file.last_modified().unwrap_or_default());
        let mut data = Vec::new();
        (&mut file)
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|e| part_error(e.to_string()))?;
        if data.len() as u64 > limit {
            return Err(ScrubError::TooLarge(name.clone()));
        }
        drop(file);
        let new = match entry {
            Entry::Text(rules) => {
                // The importer counts function words in the body only.
                sc.set_lang_budget(if key(name) == key(&at.main) {
                    LANG_BUDGET
                } else {
                    0
                });
                scrub_part(&data, rules, &sc).map_err(part_error)?
            }
            Entry::Picture => image::blank(&data).map_err(|why| ScrubError::Picture {
                name: name.clone(),
                why,
            })?,
            Entry::Copy => data,
        };
        out.start_file(name.as_str(), options)
            .map_err(|e| part_error(e.to_string()))?;
        out.write_all(&new).map_err(|e| part_error(e.to_string()))?;
    }
    let cursor = out
        .finish()
        .map_err(|e| ScrubError::NotDocx(e.to_string()))?;
    Ok(cursor.into_inner())
}

fn entry_error(i: usize, e: &dyn std::fmt::Display) -> ScrubError {
    ScrubError::NotDocx(format!("entry {i} of the zip cannot be read ({e})"))
}

/// An XML part's bytes, scrambled, in the encoding it came in.
fn scrub_part(data: &[u8], rules: &Rules, sc: &Scrambler) -> Result<Vec<u8>, String> {
    enum Enc {
        Utf8,
        Utf8Bom,
        Utf16(bool),
    }
    let utf16 = |rest: &[u8], le: bool| -> Result<String, String> {
        let (pairs, odd) = rest.as_chunks::<2>();
        if !odd.is_empty() {
            return Err("UTF-16 with an odd number of bytes".to_string());
        }
        let units: Vec<u16> = pairs
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| "not valid UTF-16".to_string())
    };
    let (text, enc) = match data {
        [0xff, 0xfe, rest @ ..] => (utf16(rest, true)?, Enc::Utf16(true)),
        [0xfe, 0xff, rest @ ..] => (utf16(rest, false)?, Enc::Utf16(false)),
        [0xef, 0xbb, 0xbf, rest @ ..] => (
            String::from_utf8(rest.to_vec()).map_err(|_| "not valid UTF-8")?,
            Enc::Utf8Bom,
        ),
        _ => (
            String::from_utf8(data.to_vec()).map_err(|_| "not valid UTF-8")?,
            Enc::Utf8,
        ),
    };
    let new = xml::scrub(&text, rules, sc)?;
    Ok(match enc {
        Enc::Utf8 => new.into_bytes(),
        Enc::Utf8Bom => [&[0xef, 0xbb, 0xbf][..], new.as_bytes()].concat(),
        Enc::Utf16(le) => {
            let mut out = if le {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for u in new.encode_utf16() {
                out.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            out
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_sorted_into_copy_text_and_picture() {
        let at = Located {
            main: "word/document.xml".into(),
            core: "docProps/core.xml".into(),
            app: "docProps/app.xml".into(),
        };
        let kind = |n: &str| match classify(n, &at) {
            Entry::Copy => "copy",
            Entry::Text(r) if std::ptr::eq(r, &WORD) => "word",
            Entry::Text(r) if std::ptr::eq(r, &CORE) => "core",
            Entry::Text(_) => "app",
            Entry::Picture => "picture",
        };
        for (name, want) in [
            ("[Content_Types].xml", "copy"),
            ("_rels/.rels", "copy"),
            ("word/_rels/document.xml.rels", "copy"),
            ("word/document.xml", "word"),
            ("word/footnotes.xml", "word"),
            ("word/header1.xml", "word"),
            ("word/comments.xml", "word"),
            ("word/charts/chart1.xml", "word"),
            ("word/styles.xml", "copy"),
            ("word/settings.xml", "copy"),
            ("/word/header2.xml", "word"),
            ("word/numbering.xml", "copy"),
            ("word/fontTable.xml", "copy"),
            ("word/theme/theme1.xml", "copy"),
            ("docProps/core.xml", "core"),
            ("docProps/app.xml", "app"),
            ("docProps/thumbnail.jpeg", "picture"),
            ("word/media/image1.PNG", "picture"),
            ("word/media/hdphoto1.wdp", "picture"),
            ("word/media/image2.heic", "picture"),
            ("word/media/_rels/image1.xml.rels", "copy"),
            ("word/embeddings/oleObject1.bin", "copy"),
            ("customXml/item1.xml", "word"),
            ("customXml/itemProps1.xml", "word"),
            ("docProps/custom.xml", "word"),
            ("word/theme/theme1.xml", "copy"),
            ("doc/media/hdphoto1.wdp", "picture"),
            ("doc/styles.xml", "copy"),
            ("doc/header1.xml", "word"),
            ("xl/sharedStrings.xml", "word"),
        ] {
            assert_eq!(kind(name), want, "{name}");
        }
    }

    #[test]
    fn enumerated_and_code_text_comes_back_unchanged() {
        // The importer reads `wp:align` ("left", "right", "inside", ...) to
        // place a picture; Word reads the OLE and chart values. None is
        // prose, and the prose beside them is still scrambled.
        let xml = format!(
            r#"<w:document {NS} xmlns:wp="w" xmlns:o="o" xmlns:c="c"><w:body><w:p><w:r><w:drawing><wp:anchor><wp:positionH relativeFrom="margin"><wp:align>left</wp:align></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:align>top</wp:align></wp:positionV></wp:anchor></w:drawing></w:r><w:r><w:t>Ein Landarzt</w:t></w:r><w:r><w:object><o:OLEObject><o:LinkType>EnhancedMetaFile</o:LinkType><o:LockedField>false</o:LockedField><o:FieldCodes>\f 0 \s</o:FieldCodes></o:OLEObject></w:object></w:r></w:p><c:numFmt><c:formatCode>0.00%</c:formatCode><c:f>Sheet1!$B$2:$B$5</c:f></c:numFmt><b:Source xmlns:b="b"><b:Guid>{{3B5D6E2A-1F4C-4C8A-9E7B-2D1A0F3C4B5E}}</b:Guid><b:LCID>en-GB</b:LCID></b:Source></w:body></w:document>"#
        );
        let out = scrub_part(xml.as_bytes(), &WORD, &Scrambler::new(3)).unwrap();
        let out = String::from_utf8(out).unwrap();
        for kept in [
            "<wp:align>left</wp:align>",
            "<wp:align>top</wp:align>",
            "<o:LinkType>EnhancedMetaFile</o:LinkType>",
            "<o:LockedField>false</o:LockedField>",
            "<o:FieldCodes>\\f 0 \\s</o:FieldCodes>",
            "<c:formatCode>0.00%</c:formatCode>",
            "<c:f>Sheet1!$B$2:$B$5</c:f>",
            "<b:Guid>{3B5D6E2A-1F4C-4C8A-9E7B-2D1A0F3C4B5E}</b:Guid>",
            "<b:LCID>en-GB</b:LCID>",
        ] {
            assert!(out.contains(kept), "{kept}\n{out}");
        }
        assert!(!out.contains("Landarzt"));
    }

    #[test]
    fn utf16_parts_come_back_as_utf16() {
        let xml = "<w:p><w:r><w:t>Ein Landarzt</w:t></w:r></w:p>";
        let mut src = vec![0xff, 0xfe];
        for u in xml.encode_utf16() {
            src.extend(u.to_le_bytes());
        }
        let out = scrub_part(&src, &WORD, &Scrambler::new(1)).unwrap();
        assert_eq!(&out[..2], &[0xff, 0xfe]);
        assert_eq!(out.len(), src.len());
        let units: Vec<u16> = out[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        let text = String::from_utf16(&units).unwrap();
        assert!(text.starts_with("<w:p><w:r><w:t>") && !text.contains("Landarzt"));
    }

    #[test]
    fn bad_encodings_are_errors() {
        assert!(scrub_part(&[b'<', 0xff, b'>'], &WORD, &Scrambler::new(1)).is_err());
    }

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    /// A .docx with a heading, split runs, tracked changes, a field, a
    /// comment, a header and document properties, in Kafka's words.
    fn kafka_docx() -> Vec<u8> {
        let document = format!(
            r#"<w:document {NS}><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:bookmarkStart w:id="0" w:name="Der_neue_Advokat"/><w:r><w:t>Der neue Advokat</w:t></w:r><w:bookmarkEnd w:id="0"/></w:p>
<w:p><w:r><w:t xml:space="preserve">Wir haben einen neuen Advo</w:t></w:r><w:r><w:t>katen, den Dr. Bucephalus.</w:t></w:r><w:ins w:id="1" w:author="Franz Kafka" w:date="2026-10-28T10:00:00Z"><w:r><w:t xml:space="preserve"> In seinem Äußern</w:t></w:r></w:ins><w:del w:id="2" w:author="Franz Kafka" w:date="2026-10-28T10:00:00Z"><w:r><w:delText>erinnert wenig</w:delText></w:r></w:del><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "https://example.com/Bucephalus" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Gerichtsdiener</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
<w:sectPr><w:pgSz w:w="8505" w:h="12756"/></w:sectPr></w:body></w:document>"#
        );
        let comments = format!(
            r#"<w:comments {NS}><w:comment w:id="0" w:author="Franz Kafka" w:initials="FK"><w:p><w:r><w:t>Doch sah ich letzthin selbst einen ganz einfältigen Gerichtsdiener</w:t></w:r></w:p></w:comment></w:comments>"#
        );
        let header =
            format!(r#"<w:hdr {NS}><w:p><w:r><w:t>Ein Landarzt</w:t></w:r></w:p></w:hdr>"#);
        let core = r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="d" xmlns:dcterms="t"><dc:title>Ein Landarzt</dc:title><dc:creator>Franz Kafka</dc:creator><dc:language>de</dc:language><dcterms:created>2026-10-28T10:00:00Z</dcterms:created></cp:coreProperties>"#;
        let app = r#"<Properties><Application>Microsoft Office Word</Application><Company>Kurt Wolff Verlag</Company></Properties>"#;
        let styles = format!(
            r#"<w:styles {NS}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style></w:styles>"#
        );
        let rels = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
        let doc_rels = r#"<Relationships><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
        crate::docx::tests::zip_of(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", rels.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
            ("word/comments.xml", comments.as_bytes()),
            ("word/header1.xml", header.as_bytes()),
            ("docProps/core.xml", core.as_bytes()),
            ("docProps/app.xml", app.as_bytes()),
        ])
    }

    fn entry(zip: &[u8], name: &str) -> String {
        let mut z = ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn text_names_and_properties_change_and_the_rest_stays() {
        let src = kafka_docx();
        let out = scrub(&src).unwrap();
        let (a, b) = (
            entry(&src, "word/document.xml"),
            entry(&out, "word/document.xml"),
        );
        // Kept: markup, style and bookmark names, the field's code, dates,
        // the page setup.
        for kept in [
            "w:name=\"Der_neue_Advokat\"",
            "w:pStyle w:val=\"Heading1\"",
            "HYPERLINK \"https://example.com/Bucephalus\"",
            "w:date=\"2026-10-28T10:00:00Z\"",
            "<w:sectPr><w:pgSz w:w=\"8505\" w:h=\"12756\"/></w:sectPr>",
            "<w:fldChar w:fldCharType=\"separate\"/>",
        ] {
            assert!(a.contains(kept) && b.contains(kept), "{kept}\n{b}");
        }
        // Changed: every word of prose, in the text, the deleted text, the
        // field's result and the people's names.
        for gone in [
            ">Der neue Advokat<",
            "Wir haben",
            "katen",
            "Äußern",
            "erinnert",
            "Gerichtsdiener",
            "Franz",
            "Kafka\"",
        ] {
            assert!(a.contains(gone), "{gone}");
            assert!(!b.contains(gone), "{gone}\n{b}");
        }
        // The word split over two runs is one word, so it has one spelling.
        assert!(!b.contains("Advo<") && b.matches("</w:t>").count() == a.matches("</w:t>").count());
        // Comments and headers: prose is gone, structure and names of
        // people changed.
        let comments = entry(&out, "word/comments.xml");
        assert!(
            comments.contains("<w:comment w:id=\"0\" w:author=\"")
                && !comments.contains("einfältigen")
        );
        assert!(!comments.contains("Kafka") && !comments.contains("\"FK\""));
        assert!(!entry(&out, "word/header1.xml").contains("Landarzt"));
        // Properties: title, author, company; not the language or the date.
        let core = entry(&out, "docProps/core.xml");
        assert!(!core.contains("Landarzt") && !core.contains("Kafka"));
        assert!(
            core.contains("<dc:language>de</dc:language>") && core.contains("2026-10-28T10:00:00Z")
        );
        let app = entry(&out, "docProps/app.xml");
        assert!(!app.contains("Wolff") && app.contains("Microsoft Office Word"));
        // Copied as they were.
        for name in [
            "word/styles.xml",
            "word/_rels/document.xml.rels",
            "_rels/.rels",
            "[Content_Types].xml",
        ] {
            assert_eq!(entry(&src, name), entry(&out, name), "{name}");
        }
    }

    /// `kafka_docx` with its parts under `doc/` instead of `word/`: the
    /// importer follows the relationships, so it reads this one.
    fn moved_docx() -> Vec<u8> {
        let document = format!(
            r#"<w:document {NS}><w:body><w:p><w:r><w:t>Der neue Advokat</w:t></w:r></w:p><w:p><w:r><w:t>Wir haben einen neuen Advokaten, den Dr. Bucephalus.</w:t></w:r></w:p></w:body></w:document>"#
        );
        let header =
            format!(r#"<w:hdr {NS}><w:p><w:r><w:t>Ein Landarzt</w:t></w:r></w:p></w:hdr>"#);
        let custom = r#"<Properties xmlns:vt="v"><property name="Titel"><vt:lpwstr>Ein Landarzt</vt:lpwstr></property><property name="Stand"><vt:filetime>2026-10-28T10:00:00Z</vt:filetime></property></Properties>"#;
        let item = r#"<root xmlns="urn:cover"><Abstract>Doch sah ich letzthin selbst einen ganz einfältigen Gerichtsdiener</Abstract><CompanyAddress>Kurt Wolff Verlag</CompanyAddress></root>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="doc/document.xml"/></Relationships>"#;
        crate::docx::tests::zip_of(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", rels.as_bytes()),
            ("doc/document.xml", document.as_bytes()),
            ("doc/header1.xml", header.as_bytes()),
            ("docProps/custom.xml", custom.as_bytes()),
            ("customXml/item1.xml", item.as_bytes()),
        ])
    }

    #[test]
    fn a_main_part_outside_word_is_scrambled_like_any_other() {
        let src = moved_docx();
        // The importer reads it.
        assert!(crate::docx::import(&src).is_ok());
        let out = scrub(&src).unwrap();
        let all: String = [
            "doc/document.xml",
            "doc/header1.xml",
            "docProps/custom.xml",
            "customXml/item1.xml",
        ]
        .iter()
        .map(|n| entry(&out, n))
        .collect();
        for gone in [
            "Advokat",
            "Bucephalus",
            "Landarzt",
            "Gerichtsdiener",
            "Wolff",
            "letzthin",
        ] {
            assert!(!all.contains(gone), "{gone}\n{all}");
        }
        // Markup, property names and the typed value stay.
        let custom = entry(&out, "docProps/custom.xml");
        assert!(
            custom.contains("<property name=\"Titel\">")
                && custom.contains("<vt:filetime>2026-10-28T10:00:00Z</vt:filetime>")
        );
        assert_eq!(entry(&src, "_rels/.rels"), entry(&out, "_rels/.rels"));
        // And the copy still imports to the same book.
        let (a, b) = (
            crate::docx::import(&src).unwrap(),
            crate::docx::import(&out).unwrap(),
        );
        assert_eq!(a.warnings, b.warnings);
        assert_eq!(a.book.sections.len(), b.book.sections.len());
    }

    #[test]
    fn a_zip_that_is_no_word_document_has_no_copy() {
        // Another Office format under a .docx name: the importer refuses it,
        // and the scrubber must not hand back its text.
        let xlsx = crate::docx::tests::zip_of(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("xl/workbook.xml", b"<workbook/>"),
            (
                "xl/sharedStrings.xml",
                b"<sst><si><t>Doch sah ich letzthin</t></si></sst>",
            ),
        ]);
        assert!(matches!(scrub(&xlsx), Err(ScrubError::NotDocx(_))));
        // A main part the relationship names but the zip does not hold.
        let rels = br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="doc/document.xml"/></Relationships>"#;
        let missing = crate::docx::tests::zip_of(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", rels),
        ]);
        assert!(matches!(scrub(&missing), Err(ScrubError::NotDocx(_))));
    }

    #[test]
    fn the_title_page_name_and_the_document_author_still_agree() {
        // The importer takes a title-page line equal to dc:creator as the
        // author; a word is replaced alike everywhere, so it still can.
        let (title, creator) = (
            entry(&scrub(&kafka_docx()).unwrap(), "docProps/core.xml"),
            entry(&scrub(&kafka_docx()).unwrap(), "word/document.xml"),
        );
        let in_core = title
            .split("<dc:creator>")
            .nth(1)
            .unwrap()
            .split('<')
            .next()
            .unwrap()
            .to_string();
        let in_doc: Vec<&str> = creator
            .split("w:author=\"")
            .skip(1)
            .map(|s| s.split('"').next().unwrap())
            .collect();
        assert!(
            in_doc.iter().all(|a| *a == in_core),
            "{in_core} / {in_doc:?}"
        );
    }
}
