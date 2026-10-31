//! .docx import.
//!
//! Layers, bottom up: `package` (zip, relationships, size limits), `xml`
//! (attribute helpers), `styles` (style chains, numbering), `story`
//! (streaming parse into paragraphs with resolved formatting), `media`
//! (images), `inline` (segments → model inlines), `structure` (chapters,
//! front matter, scene breaks, metadata).

mod inline;
mod media;
mod package;
mod pictures;
mod story;
mod structure;
mod styles;
mod table;
mod xml;

use crate::{ImportError, ImportWarning, Imported};
use media::Media;
use package::{Package, RelKind};
use quick_xml::Reader;
use quick_xml::events::Event;
use setzer_model::{Block, Book};
use std::collections::HashMap;
use story::{Env, NoteKind, Para};
use structure::Core;
use styles::{Numbering, Styles};

/// The relationship target of a kind, for internal targets only.
fn internal(rels: &HashMap<String, package::Rel>, kind: RelKind) -> Option<String> {
    rels.values()
        .find(|r| r.kind == kind && !r.external)
        .map(|r| r.target.clone())
}

/// The main document part: the `officeDocument` relationship's target, or
/// `word/document.xml` when there is none.
fn main_part(pkg: &Package, root: &HashMap<String, package::Rel>) -> Result<String, ImportError> {
    internal(root, RelKind::OfficeDocument)
        .filter(|p| pkg.has(p))
        .or_else(|| {
            pkg.has("word/document.xml")
                .then(|| "word/document.xml".into())
        })
        .ok_or_else(|| ImportError::NotDocx("it has no main document part".into()))
}

/// The parts of a package that the importer finds through relationships
/// (in the case the relationship spells them; entry names are matched
/// without regard to case). The scrambled copy asks the same question, so
/// it never reads a different part than the importer does.
pub(crate) struct Located {
    /// The main document.
    pub main: String,
    /// The core properties (title, author).
    pub core: String,
    /// The extended properties (company, manager).
    pub app: String,
}

/// Where the importer would look for the main part and the properties.
pub(crate) fn locate(bytes: &[u8]) -> Result<Located, ImportError> {
    let mut pkg = Package::open(bytes)?;
    let root = pkg.rels("");
    Ok(Located {
        main: main_part(&pkg, &root)?,
        core: internal(&root, RelKind::CoreProperties)
            .unwrap_or_else(|| "docProps/core.xml".into()),
        app: internal(&root, RelKind::ExtendedProperties)
            .unwrap_or_else(|| "docProps/app.xml".into()),
    })
}

/// Import a .docx manuscript. Fails only if the bytes are not a Word
/// document or contain no text; everything odd inside is a warning.
pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let mut pkg = Package::open(bytes)?;
    let root = pkg.rels("");
    let main = main_part(&pkg, &root)?;
    let xml = match pkg.read_xml(&main) {
        Some(Ok(x)) => x,
        Some(Err(e)) => return Err(ImportError::NotDocx(e)),
        None => return Err(ImportError::NotDocx(format!("{main} is missing"))),
    };

    let core_path =
        internal(&root, RelKind::CoreProperties).unwrap_or_else(|| "docProps/core.xml".into());
    let core = match pkg.read_xml(&core_path) {
        Some(Ok(x)) => parse_core(&x),
        _ => Core::default(),
    };
    let rels = pkg.rels(&main);
    let mut read_part = |kind| {
        internal(&rels, kind)
            .and_then(|p| pkg.read_xml(&p))
            .and_then(Result::ok)
    };
    let styles = read_part(RelKind::Styles)
        .map(|x| Styles::parse(&x))
        .unwrap_or_default();
    let numbering = read_part(RelKind::Numbering)
        .map(|x| Numbering::parse(&x))
        .unwrap_or_default();

    let mut media = Media::default();
    let mut warnings = Vec::new();
    let no_notes = HashMap::new();
    let mut notes: HashMap<(NoteKind, i64), Vec<Block>> = HashMap::new();
    for kind in [RelKind::Footnotes, RelKind::Endnotes] {
        let Some(path) = internal(&rels, kind) else {
            continue;
        };
        let Some(Ok(x)) = pkg.read_xml(&path) else {
            continue;
        };
        let note_rels = pkg.rels(&path);
        let before = warnings.len();
        let paras = story::parse(
            &x,
            &mut Env {
                pkg: &mut pkg,
                styles: &styles,
                numbering: &numbering,
                rels: &note_rels,
                notes: &no_notes,
                media: &mut media,
                warnings: &mut warnings,
            },
        );
        // Paragraph numbers of the notes story mean nothing to the author.
        let label = if kind == RelKind::Footnotes {
            "In a footnote"
        } else {
            "In an endnote"
        };
        for w in &mut warnings[before..] {
            w.location = 0;
            if let Some((_, rest)) = w.message.split_once(": ")
                && w.message.starts_with("Paragraph ")
            {
                w.message = format!("{label}: {rest}");
            }
        }
        for mut p in paras {
            let Some(key) = p.note else { continue };
            inline::normalize(&mut p.segs);
            notes
                .entry(key)
                .or_default()
                .extend(structure::note_blocks(&p));
        }
    }

    let body_start = warnings.len();
    let paras: Vec<Para> = story::parse(
        &xml,
        &mut Env {
            pkg: &mut pkg,
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            notes: &notes,
            media: &mut media,
            warnings: &mut warnings,
        },
    );
    if !paras.iter().any(Para::is_visible) {
        return Err(ImportError::Empty);
    }
    let s = structure::build(paras, styles.lang.as_deref(), &core, &mut warnings);
    attach_sections(&mut warnings[body_start..], &s.starts);

    let mut book = Book::new(s.metadata);
    book.sections = s.sections;
    Ok(Imported {
        book,
        assets: media.assets,
        warnings,
    })
}

/// Fill `section` for warnings about a body paragraph from the paragraph
/// where each section starts.
fn attach_sections(warnings: &mut [ImportWarning], starts: &[usize]) {
    for w in warnings {
        if w.section.is_none() && w.location > 0 {
            w.section = starts.partition_point(|&s| s <= w.location).checked_sub(1);
        }
    }
}

/// `docProps/core.xml`: title, author, language.
fn parse_core(xml: &str) -> Core {
    let mut core = Core::default();
    let mut reader = Reader::from_str(xml);
    let mut field: Option<&mut Option<String>> = None;
    let mut buf = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                buf.clear();
                field = match e.local_name().into_inner() {
                    "title" => Some(&mut core.title),
                    "creator" => Some(&mut core.creator),
                    "language" => Some(&mut core.language),
                    _ => None,
                };
            }
            Ok(Event::Text(t)) if field.is_some() => buf.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) if field.is_some() => {
                if let Some(c) = xml::resolve_ref(&r) {
                    buf.push(c);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(f) = field.take() {
                    let v = buf.trim();
                    if !v.is_empty() && f.is_none() {
                        *f = Some(v.to_string());
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    core
}

#[cfg(test)]
pub(crate) mod tests;
