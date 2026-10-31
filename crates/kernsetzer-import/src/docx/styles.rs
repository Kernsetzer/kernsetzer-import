//! `word/styles.xml` and `word/numbering.xml`: style chains (`basedOn`),
//! run and paragraph properties, and what a style means for structure.

use super::xml::{attr, int_attr, on_off, val};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

/// Run properties as written at one level (style or direct formatting).
/// `None` means "not set here, inherit".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RunProps {
    pub italic: Option<bool>,
    pub bold: Option<bool>,
    pub small_caps: Option<bool>,
    pub caps: Option<bool>,
    pub hidden: Option<bool>,
    pub underline: Option<bool>,
    pub vert: Option<VertAlign>,
    /// Font size in half-points (`w:sz`).
    pub size: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VertAlign {
    Baseline,
    Super,
    Sub,
}

impl RunProps {
    /// Overlay `child` on `self` (child wins where set).
    fn overlay(self, child: RunProps) -> RunProps {
        RunProps {
            italic: child.italic.or(self.italic),
            bold: child.bold.or(self.bold),
            small_caps: child.small_caps.or(self.small_caps),
            caps: child.caps.or(self.caps),
            hidden: child.hidden.or(self.hidden),
            underline: child.underline.or(self.underline),
            vert: child.vert.or(self.vert),
            size: child.size.or(self.size),
        }
    }

    /// Apply one child element of `<w:rPr>`. Returns whether it was a run
    /// property.
    pub fn apply(&mut self, name: &str, e: &BytesStart) -> bool {
        match name {
            "i" => self.italic = Some(on_off(e)),
            "b" => self.bold = Some(on_off(e)),
            "smallCaps" => self.small_caps = Some(on_off(e)),
            "caps" => self.caps = Some(on_off(e)),
            "vanish" => self.hidden = Some(on_off(e)),
            "u" => self.underline = Some(on_off(e)),
            "sz" => self.size = int_attr(e, "val").and_then(|v| u16::try_from(v).ok()),
            "vertAlign" => {
                self.vert = Some(match val(e).as_deref() {
                    Some("superscript") => VertAlign::Super,
                    Some("subscript") => VertAlign::Sub,
                    _ => VertAlign::Baseline,
                })
            }
            _ => return false,
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// Paragraph properties at one level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ParaProps {
    pub align: Option<Align>,
    /// 0 = Heading 1 … 8; 9 means body text.
    pub outline: Option<u8>,
    pub page_break_before: Option<bool>,
    /// Indents in twips (1/1440 in).
    pub ind_left: Option<i64>,
    pub ind_right: Option<i64>,
    /// The first line's indent relative to `ind_left`: `firstLine`, or
    /// minus `hanging`.
    pub ind_first: Option<i64>,
    /// Space before and after the paragraph in twips.
    pub space_before: Option<i64>,
    pub space_after: Option<i64>,
    /// `numId`, 0 = explicitly not numbered.
    pub num_id: Option<i64>,
    pub num_level: Option<u8>,
}

impl ParaProps {
    pub fn overlay(&self, child: &ParaProps) -> ParaProps {
        ParaProps {
            align: child.align.or(self.align),
            outline: child.outline.or(self.outline),
            page_break_before: child.page_break_before.or(self.page_break_before),
            ind_left: child.ind_left.or(self.ind_left),
            ind_right: child.ind_right.or(self.ind_right),
            ind_first: child.ind_first.or(self.ind_first),
            space_before: child.space_before.or(self.space_before),
            space_after: child.space_after.or(self.space_after),
            num_id: child.num_id.or(self.num_id),
            num_level: child.num_level.or(self.num_level),
        }
    }

    /// Apply one element found inside `<w:pPr>` (including `numPr`
    /// children). Returns whether it was a paragraph property.
    pub fn apply(&mut self, name: &str, e: &BytesStart) -> bool {
        match name {
            "jc" => {
                self.align = Some(match val(e).as_deref() {
                    Some("center") => Align::Center,
                    Some("right" | "end") => Align::End,
                    Some(
                        "both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida",
                    ) => Align::Justify,
                    _ => Align::Start,
                })
            }
            "outlineLvl" => {
                self.outline = val(e).and_then(|v| v.parse().ok());
            }
            "pageBreakBefore" => self.page_break_before = Some(on_off(e)),
            "ind" => {
                if let Some(l) = int_attr(e, "left").or_else(|| int_attr(e, "start")) {
                    self.ind_left = Some(l);
                }
                if let Some(r) = int_attr(e, "right").or_else(|| int_attr(e, "end")) {
                    self.ind_right = Some(r);
                }
                if let Some(h) = int_attr(e, "hanging") {
                    self.ind_first = Some(h.saturating_neg());
                } else if let Some(f) = int_attr(e, "firstLine") {
                    self.ind_first = Some(f);
                }
            }
            "spacing" => {
                if let Some(b) = int_attr(e, "before") {
                    self.space_before = Some(b);
                }
                if let Some(a) = int_attr(e, "after") {
                    self.space_after = Some(a);
                }
            }
            "numId" => self.num_id = int_attr(e, "val"),
            "ilvl" => self.num_level = int_attr(e, "val").and_then(|v| u8::try_from(v).ok()),
            _ => return false,
        }
        true
    }
}

/// What a paragraph style says about structure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Role {
    #[default]
    Body,
    Title,
    Subtitle,
    /// Heading level, 0 = Heading 1.
    Heading(u8),
    Quote,
    Verse,
    Caption,
    /// Entries of a generated table of contents.
    Toc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Paragraph,
    Character,
    Other,
}

#[derive(Debug, Clone)]
struct Style {
    kind: StyleKind,
    name: String,
    based_on: Option<String>,
    ppr: ParaProps,
    rpr: RunProps,
}

/// What a style named for one front-matter element says the paragraph is.
/// The author chose the style, so the structure pass takes it at its word
/// where it would otherwise guess from length and wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    /// "Epigraph" or "Motto" (still a `Role::Quote` everywhere else).
    Epigraph,
    /// "Dedication" or "Widmung".
    Dedication,
}

/// A paragraph style with its chain resolved.
#[derive(Debug, Clone, Default)]
pub(super) struct ParaStyle {
    pub role: Role,
    pub purpose: Option<Purpose>,
    pub ppr: ParaProps,
    pub rpr: RunProps,
}

#[derive(Debug, Default)]
pub(super) struct Styles {
    styles: HashMap<String, Style>,
    default_para: Option<String>,
    doc_rpr: RunProps,
    doc_ppr: ParaProps,
    /// Document default language (`w:docDefaults/w:rPrDefault/w:lang`).
    pub lang: Option<String>,
}

impl Styles {
    pub fn parse(xml: &str) -> Styles {
        let mut s = Styles::default();
        let mut reader = Reader::from_str(xml);
        let mut cur: Option<(String, Style)> = None;
        // Which property block we are in: 'r' (rPr), 'p' (pPr) or none.
        let mut block: Option<char> = None;
        let mut in_defaults = false;
        // Depth of a subtree we ignore (table style conditions, revisions).
        let mut skip = 0usize;
        // A damaged styles part yields the styles read so far.
        while let Ok(ev) = reader.read_event() {
            let (e, empty) = match ev {
                Event::Start(e) => (e, false),
                Event::Empty(e) => (e, true),
                Event::End(e) => {
                    let name = e.local_name().into_inner();
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    match name {
                        "rPr" | "pPr" => block = None,
                        "docDefaults" => in_defaults = false,
                        "style" => {
                            if let Some((id, st)) = cur.take() {
                                s.styles.insert(id, st);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                Event::Eof => break,
                _ => continue,
            };
            if skip > 0 {
                if !empty {
                    skip += 1;
                }
                continue;
            }
            let name = e.local_name().into_inner();
            match name {
                "tblStylePr" | "rPrChange" | "pPrChange" | "tblPr" | "trPr" | "tcPr" => {
                    if !empty {
                        skip = 1;
                    }
                }
                "docDefaults" => in_defaults = !empty,
                "style" => {
                    let kind = match attr(&e, "type").as_deref() {
                        Some("paragraph") => StyleKind::Paragraph,
                        Some("character") => StyleKind::Character,
                        _ => StyleKind::Other,
                    };
                    let id = attr(&e, "styleId").unwrap_or_default();
                    if kind == StyleKind::Paragraph
                        && attr(&e, "default").is_some_and(|d| d == "1" || d == "true")
                    {
                        s.default_para = Some(id.clone());
                    }
                    let st = Style {
                        kind,
                        name: String::new(),
                        based_on: None,
                        ppr: ParaProps::default(),
                        rpr: RunProps::default(),
                    };
                    if empty {
                        s.styles.insert(id, st);
                    } else {
                        cur = Some((id, st));
                    }
                }
                "name" if block.is_none() => {
                    if let Some((_, st)) = &mut cur {
                        st.name = val(&e).unwrap_or_default();
                    }
                }
                "basedOn" if block.is_none() => {
                    if let Some((_, st)) = &mut cur {
                        st.based_on = val(&e);
                    }
                }
                "rPr" => block = (!empty).then_some('r'),
                "pPr" => block = (!empty).then_some('p'),
                "lang" if in_defaults && block == Some('r') => {
                    s.lang = val(&e).filter(|l| !l.is_empty());
                }
                _ => match (block, &mut cur) {
                    (Some('r'), Some((_, st))) => {
                        st.rpr.apply(name, &e);
                    }
                    (Some('p'), Some((_, st))) => {
                        st.ppr.apply(name, &e);
                    }
                    (Some('r'), None) if in_defaults => {
                        s.doc_rpr.apply(name, &e);
                    }
                    (Some('p'), None) if in_defaults => {
                        s.doc_ppr.apply(name, &e);
                    }
                    _ => {}
                },
            }
        }
        s
    }

    /// Style ids from `id` up the `basedOn` chain (cycle-safe).
    fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out: Vec<&Style> = Vec::new();
        let mut next = Some(id);
        while let Some(i) = next {
            let Some(st) = self.styles.get(i) else { break };
            if out.len() > 32 || out.iter().any(|s| std::ptr::eq(*s, st)) {
                break;
            }
            out.push(st);
            next = st.based_on.as_deref();
        }
        out
    }

    /// Resolve a paragraph style (`None` = the default paragraph style).
    pub fn para(&self, id: Option<&str>) -> ParaStyle {
        let id = id.or(self.default_para.as_deref()).unwrap_or("Normal");
        let chain = self.chain(id);
        let mut ppr = self.doc_ppr.clone();
        let mut rpr = self.doc_rpr;
        for st in chain.iter().rev() {
            ppr = ppr.overlay(&st.ppr);
            rpr = rpr.overlay(st.rpr);
        }
        let role = chain
            .iter()
            .find_map(|st| role_from_name(&st.name))
            .or(match ppr.outline {
                Some(l) if l < 9 => Some(Role::Heading(l)),
                _ => None,
            })
            .unwrap_or_default();
        let purpose = chain.iter().find_map(|st| purpose_from_name(&st.name));
        ParaStyle {
            role,
            purpose,
            ppr,
            rpr,
        }
    }

    /// Run properties of a character style, chain resolved (no defaults).
    pub fn char_rpr(&self, id: &str) -> RunProps {
        let mut rpr = RunProps::default();
        for st in self.chain(id).iter().rev() {
            if st.kind != StyleKind::Paragraph {
                rpr = rpr.overlay(st.rpr);
            }
        }
        rpr
    }
}

/// Built-in style names are stored in English in styles.xml regardless of
/// the UI language ("heading 1", "Title", "Quote"); custom styles keep the
/// author's name, so a few German names are recognized too.
fn role_from_name(name: &str) -> Option<Role> {
    let n = name.trim().to_lowercase();
    let level = |prefix: &str| {
        n.strip_prefix(prefix)
            .and_then(|r| r.trim().parse::<u8>().ok())
            .filter(|l| (1..=9).contains(l))
            .map(|l| l - 1)
    };
    if let Some(l) = level("heading").or_else(|| level("überschrift")) {
        return Some(Role::Heading(l));
    }
    if n.starts_with("toc ")
        || n.starts_with("contents ")
        || n.starts_with("verzeichnis ")
        || n == "toc heading"
        || n == "inhaltsverzeichnisüberschrift"
    {
        return Some(Role::Toc);
    }
    let role = match n.as_str() {
        "title" | "titel" => Role::Title,
        "subtitle" | "untertitel" => Role::Subtitle,
        "quote" | "intense quote" | "block text" | "quotation" | "quotations" | "block quote"
        | "blockquote" | "extract" | "epigraph" | "zitat" | "intensives zitat" | "blockzitat"
        | "motto" => Role::Quote,
        "caption" | "beschriftung" => Role::Caption,
        _ if ["verse", "poem", "poetry", "gedicht", "lyrik"]
            .iter()
            .any(|w| n.contains(w))
            && !["heading", "title", "überschrift", "titel"]
                .iter()
                .any(|w| n.contains(w)) =>
        {
            Role::Verse
        }
        _ => return None,
    };
    Some(role)
}

/// The element a style is named for, if it is one the structure pass
/// takes at its word (`Purpose`).
fn purpose_from_name(name: &str) -> Option<Purpose> {
    match name.trim().to_lowercase().as_str() {
        "epigraph" | "motto" => Some(Purpose::Epigraph),
        "dedication" | "widmung" => Some(Purpose::Dedication),
        _ => None,
    }
}

/// `word/numbering.xml`: whether a list level is bulleted or numbered.
#[derive(Debug, Default)]
pub(super) struct Numbering {
    /// numId → abstractNumId
    nums: HashMap<i64, i64>,
    /// (abstractNumId, level) → numFmt
    formats: HashMap<(i64, u8), String>,
}

impl Numbering {
    pub fn parse(xml: &str) -> Numbering {
        let mut n = Numbering::default();
        let mut reader = Reader::from_str(xml);
        let mut abs: Option<i64> = None;
        let mut level: Option<u8> = None;
        let mut num: Option<i64> = None;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) => match e.local_name().into_inner() {
                    "abstractNum" => abs = int_attr(&e, "abstractNumId"),
                    "lvl" => level = int_attr(&e, "ilvl").and_then(|v| u8::try_from(v).ok()),
                    "numFmt" => {
                        if let (Some(a), Some(l), Some(f)) = (abs, level, val(&e)) {
                            n.formats.insert((a, l), f);
                        }
                    }
                    "num" => num = int_attr(&e, "numId"),
                    "abstractNumId" => {
                        if let (Some(id), Some(a)) = (num, int_attr(&e, "val")) {
                            n.nums.insert(id, a);
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => match e.local_name().into_inner() {
                    "abstractNum" => abs = None,
                    "lvl" => level = None,
                    "num" => num = None,
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        n
    }

    /// `Some(ordered)` if the paragraph is a list item, `None` if the
    /// numbering is absent or invisible.
    pub fn list_kind(&self, num_id: i64, level: u8) -> Option<bool> {
        if num_id == 0 {
            return None;
        }
        let abs = self.nums.get(&num_id)?;
        match self.formats.get(&(*abs, level)).map(String::as_str) {
            Some("none") => None,
            Some("bullet") => Some(false),
            Some(_) => Some(true),
            // Level without a format: treat as bullets.
            None => Some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:lang w:val="de-DE" w:eastAsia="en-US"/></w:rPr></w:rPrDefault>
    <w:pPrDefault><w:pPr><w:spacing w:after="160"/></w:pPr></w:pPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Standard"><w:name w:val="Normal"/></w:style>
  <w:style w:type="paragraph" w:styleId="berschrift1"><w:name w:val="heading 1"/><w:basedOn w:val="Standard"/>
    <w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>
  <w:style w:type="paragraph" w:customStyle="1" w:styleId="Kapitel"><w:name w:val="Kapitel"/><w:basedOn w:val="berschrift1"/>
    <w:pPr><w:jc w:val="center"/></w:pPr></w:style>
  <w:style w:type="paragraph" w:styleId="Zitat"><w:name w:val="Quote"/><w:basedOn w:val="Standard"/>
    <w:pPr><w:ind w:left="864" w:right="864"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>
  <w:style w:type="character" w:styleId="Hervorhebung"><w:name w:val="Emphasis"/><w:rPr><w:i/></w:rPr></w:style>
  <w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/></w:style>
  <w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/></w:style>
  <w:style w:type="table" w:styleId="Grid"><w:name w:val="Table Grid"/>
    <w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr></w:style>
</w:styles>"#;

    #[test]
    fn resolves_chains_and_roles() {
        let s = Styles::parse(STYLES);
        assert_eq!(s.lang.as_deref(), Some("de-DE"));
        let k = s.para(Some("Kapitel"));
        assert_eq!(k.role, Role::Heading(0));
        assert_eq!(k.ppr.align, Some(Align::Center));
        assert_eq!(k.rpr.bold, Some(true));
        let q = s.para(Some("Zitat"));
        assert_eq!(q.role, Role::Quote);
        assert_eq!(q.ppr.ind_left, Some(864));
        assert_eq!(q.ppr.space_after, Some(160), "from the docDefaults");
        assert_eq!(q.rpr.italic, Some(true));
        assert_eq!(s.para(None).role, Role::Body);
        assert_eq!(s.char_rpr("Hervorhebung").italic, Some(true));
        // Cycles and unknown ids do not hang or panic.
        assert_eq!(s.para(Some("A")).role, Role::Body);
        assert_eq!(s.para(Some("nope")).role, Role::Body);
    }

    #[test]
    fn style_names() {
        assert_eq!(role_from_name("heading 2"), Some(Role::Heading(1)));
        assert_eq!(role_from_name("Überschrift 1"), Some(Role::Heading(0)));
        assert_eq!(role_from_name("toc 1"), Some(Role::Toc));
        assert_eq!(role_from_name("Contents 2"), Some(Role::Toc));
        assert_eq!(role_from_name("Intense Quote"), Some(Role::Quote));
        assert_eq!(role_from_name("Poem Stanza"), Some(Role::Verse));
        assert_eq!(role_from_name("Poem Title"), None);
        assert_eq!(role_from_name("Normal"), None);
    }

    #[test]
    fn numbering_formats() {
        let xml = r#"<w:numbering xmlns:w="w">
  <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
  <w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="none"/></w:lvl></w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;
        let n = Numbering::parse(xml);
        assert_eq!(n.list_kind(1, 0), Some(false));
        assert_eq!(n.list_kind(2, 0), Some(true));
        assert_eq!(n.list_kind(2, 1), None);
        assert_eq!(n.list_kind(0, 0), None);
        assert_eq!(n.list_kind(7, 0), None);
    }
}
