//! The structure pass: the body story's flat paragraphs → metadata and
//! sections (front matter, parts, chapters, back matter) with body blocks
//! (scene breaks, quotes, verse, lists, images).
//!
//! Headings come from heading styles when the manuscript has them ("styled
//! mode"), otherwise from layout: short centered, capitalized or bold lines
//! at a page start or after many empty paragraphs, and chapter labels
//! ("Kapitel 3", "Prolog") anywhere ("heuristic mode"). Everything before
//! the first heading is front matter, classified page by page. Empty
//! paragraphs are layout: more of them than usual between two paragraphs
//! mean a scene break.

use super::inline;
use super::story::{Para, Piece, Seg, TableData};
use super::styles::{Align, Purpose, Role};
use crate::heading::{self, HeadingInfo};
use crate::{ImportWarning, WarningKind, is_dedication, vocab};
use setzer_model::{
    Block, ColumnAlign, Epigraph, ImagePlacement, Inline, Lang, MAX_VERSE_INDENT, Metadata,
    Numbering, Section, SectionKind, Table, TableCell, TableColumn,
};
use std::ops::Range;

/// Lines longer than this (in characters) are not headings or title-page
/// lines.
const SHORT: usize = 80;
/// Longest line of a paragraph read as verse because of its line breaks.
const VERSE_LINE: usize = 60;
/// Left and right indent (twips; 567 = 1 cm) that make an unstyled
/// paragraph a block quote.
const QUOTE_INDENT: i64 = 567;
/// Space between two verse lines (twips; 20 = 1 pt) above the verse's
/// usual spacing that starts a stanza.
const STANZA_SPACE: i64 = 120;
/// Left indent (twips) that makes one verse indent level. 360 = a quarter
/// inch (0.63 cm), about the 1.5 em of 10-12 pt text that the themes give a
/// level, so the lines stand in print about as they did in the manuscript.
/// Word's own steps (the Increase Indent button, the ruler) are 720 or
/// finer, so they land on two levels or on the nearest one; a hair of ruler
/// jitter (under half a level) is none.
const VERSE_INDENT_STEP: i64 = 360;
/// Most paragraphs in a chapter epigraph, its attribution included.
const EPIGRAPH_PARAS: usize = 5;
/// Most characters in an epigraph (as for an epigraph page).
const EPIGRAPH_CHARS: usize = 600;
/// A line at least this much larger than the body text (percent) looks
/// like a heading.
const BIG_PERCENT: u32 = 120;

/// Document properties from `docProps/core.xml`.
#[derive(Debug, Default, Clone)]
pub(super) struct Core {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug)]
pub(super) struct Structured {
    pub metadata: Metadata,
    pub sections: Vec<Section>,
    /// Source paragraph number where each section starts, ascending.
    pub starts: Vec<usize>,
}

/// A visible paragraph with what its layout says.
struct Item {
    p: Para,
    /// Plain text, trimmed.
    text: String,
    starts_page: bool,
    /// Empty paragraphs before this one (0 at a page start).
    blanks: usize,
    centered: bool,
    caps: bool,
    bold: bool,
    italic: bool,
    /// Smallest font size of the text (half-points), if known.
    size: Option<u16>,
    /// Set noticeably larger than the body text.
    big: bool,
}

impl Item {
    fn short(&self) -> bool {
        self.text.chars().count() <= SHORT
    }

    fn has_image(&self) -> bool {
        self.p
            .segs
            .iter()
            .any(|s| matches!(s.piece, Piece::Image { .. }))
    }

    fn is_scene_break(&self) -> bool {
        !self.has_image() && heading::is_scene_break(&self.text)
    }

    /// Non-empty lines as plain text.
    fn line_texts(&self) -> Vec<String> {
        split_lines(&self.p.segs)
            .iter()
            .map(|l| inline::plain_text(l).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// What the text says as a heading. "Kapitel 1⏎Der Anfang" (a line
    /// break inside the heading) is a label with a title.
    fn info(&self) -> HeadingInfo {
        let lines = self.line_texts();
        if lines.len() >= 2 {
            let first = heading::classify(&lines[0]);
            if first.is_bare_label() {
                return HeadingInfo {
                    title: Some(lines[1..].join(" ")),
                    ..first
                };
            }
        }
        heading::classify(&self.text)
    }

    /// A chapter or part label, or a known section name.
    fn is_label(&self) -> bool {
        self.info().is_label()
    }

    /// Three or more short lines separated by line breaks.
    fn verse_like(&self) -> bool {
        let lines = self.line_texts();
        lines.len() >= 3 && lines.iter().all(|l| l.chars().count() <= VERSE_LINE)
    }

    /// Could be a heading by its text alone: short, not in a table or list,
    /// not an ornament, not ending like a sentence (unless it is "3.").
    fn plausible_heading(&self) -> bool {
        if self.text.is_empty()
            || !self.short()
            || self.p.in_table
            || self.p.list.is_some()
            || self.is_scene_break()
        {
            return false;
        }
        !self.text.ends_with(['.', ',', ';', ':']) || self.info().is_bare_label()
    }
}

/// What a paragraph is in the outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Body,
    /// The one top heading above all chapters: the book's title.
    BookTitle,
    Part,
    /// Starts a section (chapter, or a named section like "Prolog").
    Chapter,
    /// Heading inside a section, level from 1.
    Sub(u8),
    /// Title line after a bare chapter label.
    Title,
}

pub(super) fn build(
    paras: Vec<Para>,
    doc_lang: Option<&str>,
    core: &Core,
    warnings: &mut Vec<ImportWarning>,
) -> Structured {
    let mut toc_removed = None;
    let mut items = items(paras, &mut toc_removed);
    let g = usual_gap(&items);
    let mut marks = marks(&items, g);
    // The manuscript's closing "END" is not the book's.
    let end_mark = (marks.last() == Some(&Mark::Body)
        && items.len() >= 2
        && items.last().is_some_and(is_end_mark))
    .then(|| {
        marks.pop();
        items.pop()
    })
    .flatten();
    let mut t = Tallies::default();
    let mut fm = Front::default();
    let mut sections = Vec::new();
    let mut starts = Vec::new();

    let heads: Vec<usize> = (0..items.len())
        .filter(|&i| matches!(marks[i], Mark::Part | Mark::Chapter))
        .collect();
    if let Some(&first) = heads.first() {
        t.start_section("before the first heading".into());
        front_matter(
            &items[..first],
            &marks[..first],
            g,
            core.creator.as_deref(),
            &mut t,
            &mut fm,
            &mut sections,
            &mut starts,
            warnings,
        );
    } else if !items.is_empty() {
        // No headings: Title/Subtitle-styled lines at the top are the
        // book's title, everything else one untitled chapter.
        let mut lead = 0;
        while let Some(it) = items.get(lead) {
            match it.p.role {
                Role::Title => fm.title.get_or_insert_with(|| it.text.clone()),
                Role::Subtitle => fm.subtitle.get_or_insert_with(|| it.text.clone()),
                _ => break,
            };
            lead += 1;
        }
        if lead < items.len() {
            t.start_section(String::new());
            sections.push(Section {
                style: Default::default(),
                kind: SectionKind::Chapter,
                title: None,
                numbering: Numbering::Auto,
                epigraph: None,
                blocks: blocks(&items[lead..], &marks[lead..], g, true, &mut t),
            });
            starts.push(items[lead].p.index);
        }
        warnings.push(ImportWarning {
            kind: WarningKind::NoChapters,
            location: 0,
            section: None,
            message: "No chapter headings were found, so the whole text is one chapter. \
                      Use the Heading 1 style for chapter titles in Word."
                .into(),
            detail: None,
        });
    }

    for (h, &i) in heads.iter().enumerate() {
        let end = heads.get(h + 1).copied().unwrap_or(items.len());
        let mut body = i + 1;
        let title_line = if marks.get(i + 1) == Some(&Mark::Title) {
            body += 1;
            Some(items[i + 1].text.clone())
        } else {
            None
        };
        let info = items[i].info();
        let named = !matches!(info.kind, SectionKind::Chapter | SectionKind::Part);
        let as_part = info.kind == SectionKind::Part || (marks[i] == Mark::Part && !named);
        let (kind, title, numbering) = if as_part {
            let mut full = items[i].line_texts().join(": ");
            if let Some(t) = title_line {
                full = format!("{full}: {t}");
            }
            (SectionKind::Part, Some(full), Numbering::None)
        } else {
            let numbering = info.numbering();
            (info.kind, title_line.or(info.title), numbering)
        };
        if kind == SectionKind::TableOfContents {
            toc_removed.get_or_insert(items[i].p.index);
            continue;
        }
        let epigraph = section_epigraph(&items[body..end], &marks[body..end]);
        if let Some((_, n)) = &epigraph {
            body += n;
        }
        t.start_section(format!(
            "under the heading “{}”",
            items[i].line_texts().join(" ")
        ));
        let here = section_gap(&items[body..end], g);
        sections.push(Section {
            style: Default::default(),
            kind,
            title,
            numbering,
            epigraph: epigraph.map(|(e, _)| e),
            blocks: blocks(&items[body..end], &marks[body..end], here, true, &mut t),
        });
        starts.push(items[i].p.index);
    }

    if let Some(at) = toc_removed {
        warnings.push(ImportWarning {
            kind: WarningKind::TableOfContentsRemoved,
            location: at,
            section: None,
            message:
                "The table of contents was removed; Kernsetzer generates it from the chapters."
                    .into(),
            detail: None,
        });
    }
    t.report(warnings);
    if let Some(it) = end_mark {
        warnings.push(ImportWarning {
            kind: WarningKind::EndMarkRemoved,
            location: it.p.index,
            section: None,
            message: format!(
                "Paragraph {}: the closing “{}” under the last paragraph was left out of the book.",
                it.p.index, it.text
            ),
            detail: None,
        });
    }

    let metadata = metadata(&items, fm, doc_lang, core, warnings);
    Structured {
        metadata,
        sections,
        starts,
    }
}

/// A closing mark typed under the last paragraph of a manuscript: "END",
/// "The End", "— Ende —", "Fin." alone on its line. A title
/// page line or a heading never is one (the caller checks the mark).
fn is_end_mark(it: &Item) -> bool {
    if it.p.role != Role::Body
        || it.p.in_table
        || it.p.list.is_some()
        || it.has_image()
        || it.line_texts().len() != 1
        || it.p.segs.iter().any(|s| matches!(s.piece, Piece::Note(_)))
    {
        return false;
    }
    let words = it
        .text
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let words: Vec<&str> = words.split_whitespace().collect();
    vocab::END_MARKS.contains(&words.as_slice())
}

// ---------------------------------------------------------------- items

fn items(paras: Vec<Para>, toc_removed: &mut Option<usize>) -> Vec<Item> {
    let mut out = Vec::new();
    let mut blanks = 0;
    let mut page = true;
    for mut p in paras {
        if p.toc || p.role == Role::Toc {
            toc_removed.get_or_insert(p.index);
            page |= p.break_after;
            continue;
        }
        inline::normalize(&mut p.segs);
        if !p.is_visible() {
            if p.page_break_before || p.break_after {
                page = true;
            } else {
                blanks += 1;
            }
            continue;
        }
        let starts_page = page || p.page_break_before;
        let text = inline::plain_text(&p.segs).trim().to_string();
        let words: Vec<&Seg> = p
            .segs
            .iter()
            .filter(|s| s.text().is_some_and(|t| !t.trim().is_empty()))
            .collect();
        let all = |f: fn(&Seg) -> bool| !words.is_empty() && words.iter().all(|s| f(s));
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        let caps_by_text = letters.len() >= 2 && letters.iter().all(|c| !c.is_lowercase());
        // Word needs a paragraph after a table (and between two tables, or
        // they merge), and authors leave one before it: one empty paragraph
        // next to a table is its spacing. Two or more are a scene break by
        // the normal rule.
        let at_table = out.last().is_some_and(|prev: &Item| {
            prev.p.in_table != p.in_table || prev.p.table.is_some() || p.table.is_some()
        });
        out.push(Item {
            starts_page,
            blanks: if starts_page {
                0
            } else {
                blanks - usize::from(at_table && blanks > 0)
            },
            centered: p.align == Align::Center,
            caps: caps_by_text || all(|s| s.caps),
            bold: all(|s| s.fmt.strong),
            italic: all(|s| s.fmt.em),
            size: words
                .iter()
                .map(|s| s.size)
                .collect::<Option<Vec<u16>>>()
                .and_then(|v| v.into_iter().min()),
            big: false,
            text,
            p,
        });
        blanks = 0;
        page = out.last().is_some_and(|it: &Item| it.p.break_after);
    }
    if let Some(body) = body_size(&out) {
        for it in &mut out {
            it.big = it
                .size
                .is_some_and(|s| u32::from(s) * 100 >= u32::from(body) * BIG_PERCENT);
        }
    }
    out
}

/// The font size most of the body text is set in.
fn body_size(items: &[Item]) -> Option<u16> {
    let mut chars: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    for it in items.iter().filter(|it| it.p.role == Role::Body) {
        if let Some(s) = it.size {
            *chars.entry(s).or_default() += it.text.len();
        }
    }
    chars
        .into_iter()
        .max_by_key(|&(size, n)| (n, std::cmp::Reverse(size)))
        .map(|(size, _)| size)
}

/// The most common number of empty paragraphs between two paragraphs on
/// the same page: 0 in most manuscripts, 1 in block-style ones.
fn usual_gap(items: &[Item]) -> usize {
    gap_mode(items).0
}

/// The usual gap in one section (its paragraphs after the heading): its
/// own most common gap when at least three and most of its gaps agree,
/// else the document's. Authors change the spacing between chapters (a
/// chapter typed with empty lines between paragraphs among chapters
/// without), and a short chapter says too little to judge.
fn section_gap(items: &[Item], doc: usize) -> usize {
    let (gap, n, total) = gap_mode(items.get(1..).unwrap_or_default());
    if n >= 3 && n * 2 > total { gap } else { doc }
}

/// The most common gap, how many paragraphs have it and how many were
/// counted (those not at a page start).
fn gap_mode(items: &[Item]) -> (usize, usize, usize) {
    let mut counts = [0usize; 8];
    for it in items.iter().filter(|i| !i.starts_page) {
        counts[it.blanks.min(7)] += 1;
    }
    let gap = (0..counts.len())
        .max_by_key(|&i| (counts[i], std::cmp::Reverse(i)))
        .unwrap_or(0);
    (gap, counts[gap], counts.iter().sum())
}

// ------------------------------------------------------------- headings

fn marks(items: &[Item], g: usize) -> Vec<Mark> {
    let n = items.len();
    let mut marks = styled_marks(items).unwrap_or_else(|| vec![Mark::Body; n]);
    let styled = marks.iter().any(|m| *m != Mark::Body);

    let mut in_toc = false;
    for i in 0..n {
        let it = &items[i];
        if marks[i] != Mark::Body {
            if matches!(marks[i], Mark::Part | Mark::Chapter) {
                in_toc = it.info().kind == SectionKind::TableOfContents;
            }
            continue;
        }
        // A paragraph in a Dedication or Epigraph style is that element,
        // even when its text is a section name ("Widmung").
        if it.p.role != Role::Body || it.p.purpose.is_some() || !it.plausible_heading() {
            continue;
        }
        let label = it.is_label();
        // A large font counts like a page break: authors who format
        // headings by hand often just make them bigger.
        let gap = it.starts_page || it.blanks >= g + 3 || it.big;
        let looks = it.centered || it.caps || it.bold || it.big;
        let heading = if styled {
            // The author forgot the heading style once.
            it.starts_page && label
        } else if in_toc {
            // Inside a typed table of contents every line is a label. It
            // ends at a page start or wide gap, or at a heading with prose
            // right under it (typed contents are often followed by a few
            // empty lines only).
            (gap || prose_follows(items, i)) && (label || looks)
        } else {
            label || (looks && gap && text_follows(items, i))
        };
        if heading {
            marks[i] = Mark::Chapter;
            in_toc = it.info().kind == SectionKind::TableOfContents;
        }
    }

    // When chapters start new pages, a plain title alone at the top of a
    // page with prose under it is a heading too ("The Lantern Fair" among
    // "Chapter 1", "Chapter 2").
    let heads: Vec<bool> = (0..n)
        .filter(|&i| marks[i] == Mark::Chapter)
        .map(|i| items[i].starts_page)
        .collect();
    let on_pages = heads.iter().filter(|&&s| s).count();
    if !styled && on_pages >= 2 && on_pages * 2 > heads.len() {
        for i in 0..n {
            let it = &items[i];
            if marks[i] == Mark::Body
                && it.starts_page
                && it.p.role == Role::Body
                && it.p.purpose.is_none()
                && title_like(it)
                && prose_follows(items, i)
            {
                marks[i] = Mark::Chapter;
            }
        }
    }

    let mut title: Vec<bool> = (0..n.saturating_sub(1))
        .map(|i| {
            matches!(marks[i], Mark::Part | Mark::Chapter)
                && takes_title(items, i)
                && !dateline_under(items, &marks, i)
                && !chapter_under_part(items, &marks, i)
        })
        .collect();
    one_title_decision(items, &marks, &mut title);
    for (i, t) in title.into_iter().enumerate() {
        if t && !chapter_under_part(items, &marks, i) {
            marks[i + 1] = Mark::Title;
        }
    }
    marks
}

/// Marks from heading styles, `None` if the manuscript has none.
fn styled_marks(items: &[Item]) -> Option<Vec<Mark>> {
    let any_heading = items.iter().any(|i| matches!(i.p.role, Role::Heading(_)));
    let title_as_heading =
        !any_heading && items.iter().filter(|i| i.p.role == Role::Title).count() >= 2;
    let lvl: Vec<Option<u8>> = items
        .iter()
        .map(|it| {
            if it.text.is_empty() || it.is_scene_break() {
                return None;
            }
            match it.p.role {
                Role::Heading(l) => Some(l),
                Role::Title if title_as_heading => Some(0),
                _ => None,
            }
        })
        .collect();
    let mut levels: Vec<u8> = lvl.iter().flatten().copied().collect();
    levels.sort_unstable();
    levels.dedup();
    if levels.is_empty() {
        return None;
    }
    let count = |l: u8| lvl.iter().filter(|x| **x == Some(l)).count();
    let mut marks = vec![Mark::Body; items.len()];

    // One top heading, first, above several of the next level: the title.
    if levels.len() >= 2
        && count(levels[0]) == 1
        && lvl.iter().flatten().next() == Some(&levels[0])
        && count(levels[1]) >= 2
        && let Some(i) = lvl.iter().position(|l| *l == Some(levels[0]))
    {
        marks[i] = Mark::BookTitle;
        levels.remove(0);
    }

    let top = levels[0];
    let parts = levels.len() >= 2 && is_part_level(items, &lvl, &marks, top, levels[1]);
    let chapter = if parts { levels[1] } else { top };
    for (i, l) in lvl.iter().enumerate() {
        let Some(l) = *l else { continue };
        if marks[i] == Mark::BookTitle {
            continue;
        }
        marks[i] = if parts && l == top {
            Mark::Part
        } else if l <= chapter {
            Mark::Chapter
        } else {
            Mark::Sub(l - chapter)
        };
    }
    Some(marks)
}

/// Whether the top heading level holds parts: some are labelled as parts,
/// or most have headings of the next level under them (and they are not
/// numbered chapters with sub-headings).
fn is_part_level(items: &[Item], lvl: &[Option<u8>], marks: &[Mark], top: u8, next: u8) -> bool {
    let tops: Vec<usize> = (0..items.len())
        .filter(|&i| lvl[i] == Some(top) && marks[i] != Mark::BookTitle)
        .collect();
    let infos: Vec<HeadingInfo> = tops.iter().map(|&i| items[i].info()).collect();
    if infos.iter().any(|h| h.kind == SectionKind::Part) {
        return true;
    }
    let with_children = tops
        .iter()
        .enumerate()
        .filter(|&(k, &i)| {
            let end = tops.get(k + 1).copied().unwrap_or(items.len());
            (i + 1..end).any(|j| lvl[j] == Some(next))
        })
        .count();
    let numbered = infos
        .iter()
        .filter(|h| h.kind == SectionKind::Chapter && h.number.is_some())
        .count();
    with_children * 2 > tops.len() && numbered * 2 <= tops.len()
}

/// Body text (not centered, not an author line) follows before the next
/// page or heading-like line: a centered or capitalized line on a title or
/// dedication page is not a chapter heading.
fn text_follows(items: &[Item], i: usize) -> bool {
    items[i + 1..]
        .iter()
        .take_while(|it| {
            let heading_like =
                it.is_label() || (it.plausible_heading() && (it.big || it.bold || it.caps));
            !it.starts_page && !heading_like
        })
        .any(|it| !it.centered && !it.text.is_empty() && author_line(&it.text).is_none())
}

/// The next paragraph, on the same page, is prose: longer than any heading
/// or contents line.
fn prose_follows(items: &[Item], i: usize) -> bool {
    items
        .get(i + 1)
        .is_some_and(|n| !n.starts_page && !n.short())
}

/// A bare label ("Kapitel 3") followed by its title on the next line: set
/// off by heading formatting or size, or (unstyled label) formatted like
/// the label, not like a sentence, and with prose under it.
fn takes_title(items: &[Item], i: usize) -> bool {
    let it = &items[i];
    if !it.info().is_bare_label() || it.line_texts().len() > 1 {
        return false;
    }
    let n = &items[i + 1];
    if n.starts_page
        || !n.plausible_heading()
        || n.is_label()
        || n.text.starts_with(['—', '–', '-'])
    {
        return false;
    }
    let look = |x: &Item| (x.centered, x.bold, x.italic, x.size);
    set_off(n)
        || (it.p.role == Role::Body
            && look(it) == look(n)
            && title_like(n)
            && prose_follows(items, i + 1))
}

/// The heading under part label `i` is the part's first chapter, not its
/// title line: a chapter mark in a lower heading level than the label
/// ("Part Two" in Heading 1 over "The Letter" in Heading 2), or one with
/// no other chapter after it in the part ("Teil 2" over "Ein Landarzt",
/// both Heading 1, then text and "Teil 3"). A part holds chapters.
fn chapter_under_part(items: &[Item], marks: &[Mark], i: usize) -> bool {
    let is_part = |j: usize| marks[j] == Mark::Part || items[j].info().kind == SectionKind::Part;
    if !is_part(i) || marks.get(i + 1) != Some(&Mark::Chapter) {
        return false;
    }
    let lower = matches!(
        (items[i].p.role, items[i + 1].p.role),
        (Role::Heading(a), Role::Heading(b)) if b > a
    );
    lower
        || !(i + 2..items.len())
            .take_while(|&j| !is_part(j))
            .any(|j| marks[j] == Mark::Chapter && items[j].info().kind == SectionKind::Chapter)
}

/// Set off from body text by its style or formatting.
fn set_off(it: &Item) -> bool {
    matches!(it.p.role, Role::Heading(_) | Role::Title | Role::Subtitle)
        || it.centered
        || it.caps
        || it.bold
        || it.italic
        || it.big
}

/// One decision for the lines under the bare labels of one kind (same
/// style and look, e.g. "1", "2", … in Heading 2) when their lines share a
/// style and look too: a title under every label or under none, so a
/// final "." ("11:42 p.m.") does not decide alone. None are titles when
/// fewer than half of them are titles on their own, or at least half are
/// datelines. Lines that look like body text keep their own decision when
/// the group says titles: there the "." tells a title from a sentence.
fn one_title_decision(items: &[Item], marks: &[Mark], title: &mut [bool]) {
    type Look = (Option<String>, Role, bool, bool, bool, bool, Option<u16>);
    let look = |x: &Item| -> Look {
        let p = &x.p;
        (
            p.style.clone(),
            p.role,
            x.centered,
            x.caps,
            x.bold,
            x.italic,
            x.size,
        )
    };
    let mut groups: Vec<((Mark, Look, Look), Vec<usize>)> = Vec::new();
    for (i, w) in items.windows(2).enumerate().take(title.len()) {
        let (it, n) = (&w[0], &w[1]);
        if !matches!(marks[i], Mark::Part | Mark::Chapter)
            || !it.info().is_bare_label()
            || it.line_texts().len() > 1
            || !title_shape(n)
        {
            continue;
        }
        let key = (marks[i], look(it), look(n));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    for (_, g) in groups.iter().filter(|(_, g)| g.len() >= 2) {
        let datelines = g
            .iter()
            .filter(|&&i| dateline_under(items, marks, i))
            .count();
        let titles = g.iter().filter(|&&i| title[i]).count();
        let all = datelines * 2 < g.len() && titles * 2 >= g.len();
        for &i in g {
            title[i] = all && (title[i] || set_off(&items[i + 1]));
        }
    }
}

/// Could be a title line by its shape, a final "." allowed: short text on
/// the same page, not a label, an ornament, a list item, a dash line,
/// dialogue or an exclamation.
fn title_shape(it: &Item) -> bool {
    !it.starts_page
        && !it.text.is_empty()
        && it.short()
        && !it.p.in_table
        && it.p.list.is_none()
        && !it.is_scene_break()
        && !it.is_label()
        && !it.text.starts_with(['—', '–', '-'])
        && !it.text.ends_with([',', ';', ':'])
        && reads_like_title(it)
}

/// The line under chapter label `i` is a dateline. Under a part label
/// it is the part's title as usual ("Teil 1" + "1914–1918").
fn dateline_under(items: &[Item], marks: &[Mark], i: usize) -> bool {
    marks[i] == Mark::Chapter
        && items[i].info().kind != SectionKind::Part
        && items.get(i + 1).is_some_and(|n| dateline(&n.text))
}

/// A dateline: a time ("11:42 p.m.", "23.15 Uhr"), a date ("15 November
/// 1978", "9.11.1989", "3/14/79") or a place with a year ("Berlin, 1989",
/// "Gorran Holm — 1979"). Under a chapter label it sets the scene; it is
/// not the chapter's title.
fn dateline(text: &str) -> bool {
    let c: Vec<char> = text.to_lowercase().chars().collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let start = i;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
        }
        if i > start {
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    let word_after = |e: usize| -> String {
        let rest: String = c[e..].iter().collect();
        let w = rest.split_whitespace().next().unwrap_or_default();
        w.trim_end_matches([',', ';', ')']).to_string()
    };
    // The nearest characters before and after a run, spaces skipped.
    let before = |s: usize| c[..s].iter().rev().find(|c| !c.is_whitespace()).copied();
    let after = |e: usize| c[e..].iter().find(|c| !c.is_whitespace()).copied();
    for (k, &(s, e)) in runs.iter().enumerate() {
        let (len, sep) = (e - s, c.get(e).copied());
        if let Some(&(s2, e2)) = runs.get(k + 1)
            && len <= 2
        {
            let gap: String = c[e..s2].iter().collect();
            let gap = gap.trim();
            // 11:42, 23.15 Uhr
            let minutes = s2 == e + 1 && e2 - s2 == 2;
            if minutes
                && (sep == Some(':')
                    || (sep == Some('.') && vocab::CLOCK_WORDS.contains(&word_after(e2).as_str())))
            {
                return true;
            }
            // 9.11.1989, 9. 11. 1989, 3/14/79
            if e2 - s2 <= 2 && ((gap == "." && c.get(e2) == Some(&'.')) || gap == "/") {
                return true;
            }
        }
        // 11 p.m., 8 Uhr
        if len <= 2 && vocab::CLOCK_AFTER_HOURS.contains(&word_after(e).as_str()) {
            return true;
        }
        // Berlin, 1989 / 1989, Berlin / Gorran Holm — 1979, but not the
        // end of a range (1914–1918)
        let year = len == 4 && matches!(c[s], '1' | '2');
        let dash = matches!(before(s), Some('—' | '–' | '-'))
            && !k
                .checked_sub(1)
                .and_then(|j| runs.get(j))
                .is_some_and(|&(_, e0)| {
                    c[e0..s]
                        .iter()
                        .all(|c| c.is_whitespace() || matches!(c, '—' | '–' | '-'))
                });
        if year && (before(s) == Some(',') || dash || after(e) == Some(',')) {
            return true;
        }
    }
    let words = || {
        c.split(|c| !c.is_alphabetic())
            .map(|w| w.iter().collect::<String>())
    };
    !runs.is_empty() && words().any(|w| vocab::MONTHS.contains(&w.as_str()))
}

/// Could be a heading and does not read like dialogue or an exclamation.
fn title_like(it: &Item) -> bool {
    it.plausible_heading() && reads_like_title(it)
}

/// Does not start or end with quotes or end in "!", "?", "…".
fn reads_like_title(it: &Item) -> bool {
    const QUOTES: [char; 9] = ['"', '\'', '“', '”', '„', '‚', '‘', '’', '»'];
    !it.text.starts_with(QUOTES)
        && !it.text.ends_with(QUOTES)
        && !it.text.ends_with(['!', '?', '…'])
}

// --------------------------------------------------------- front matter

#[derive(Default)]
struct Front {
    /// From the Title / Subtitle styles.
    title: Option<String>,
    subtitle: Option<String>,
    /// The book-title heading (styled mode).
    heading_title: Option<String>,
    /// First line of an unstyled title page.
    page_title: Option<String>,
    author: Option<String>,
    copyright: Option<String>,
    /// Paragraph of the replaced title page.
    replaced: Option<usize>,
    /// Lines of the replaced title page that went nowhere.
    dropped: Vec<String>,
    /// The subtitle was read from an unstyled title page.
    page_subtitle: bool,
}

impl Front {
    /// Take an author line ("by Ada Quill"), or a line that is only the
    /// document's author ("Henrike Albrecht"), as the author.
    fn take_author(&mut self, text: &str, creator: Option<&str>) -> bool {
        let name = author_line(text).or_else(|| {
            creator
                .filter(|c| !c.trim().is_empty() && same_words(c, text))
                .map(|_| text.to_string())
        });
        let found = name.is_some();
        if let Some(a) = name {
            self.author.get_or_insert(a);
        }
        found
    }
}

/// Classify the pages before the first heading. `creator`: the
/// document's author, which a title page may name without "by".
#[allow(clippy::too_many_arguments)]
fn front_matter(
    items: &[Item],
    marks: &[Mark],
    g: usize,
    creator: Option<&str>,
    t: &mut Tallies,
    fm: &mut Front,
    sections: &mut Vec<Section>,
    starts: &mut Vec<usize>,
    warnings: &mut Vec<ImportWarning>,
) {
    let mut prev_other = false;
    for (no, r) in pages(items).into_iter().enumerate() {
        let page = &items[r.clone()];
        let pm = &marks[r.clone()];
        let at = page[0].p.index;

        // Title page from styles.
        if page.iter().zip(pm).any(|(it, m)| {
            matches!(it.p.role, Role::Title | Role::Subtitle) || *m == Mark::BookTitle
        }) {
            for (it, m) in page.iter().zip(pm) {
                let text = || it.text.clone();
                match (it.p.role, m) {
                    (Role::Title, _) => {
                        fm.title.get_or_insert_with(text);
                    }
                    (Role::Subtitle, _) => {
                        fm.subtitle.get_or_insert_with(text);
                    }
                    (_, Mark::BookTitle) => {
                        fm.heading_title.get_or_insert_with(text);
                    }
                    _ if it.p.table.is_some() => {
                        // A table has no text of its own: each row is a line.
                        for line in it.p.table.iter().flat_map(table_lines) {
                            if !contact_line(&line) {
                                fm.dropped.push(line);
                            }
                        }
                    }
                    _ => {
                        if !fm.take_author(&it.text, creator)
                            && !it.text.is_empty()
                            && !contact_line(&it.text)
                        {
                            fm.dropped.push(it.text.clone());
                        }
                    }
                }
            }
            fm.replaced.get_or_insert(at);
            prev_other = false;
            continue;
        }

        if page.iter().any(|it| is_copyright(&it.text)) {
            if fm.copyright.is_none() {
                fm.copyright = page
                    .iter()
                    .find(|it| {
                        it.text.contains('©')
                            || it.text.to_lowercase().starts_with(vocab::COPYRIGHT_LEAD)
                    })
                    .map(|it| it.text.clone());
            }
            push_section(
                sections,
                starts,
                at,
                SectionKind::Copyright,
                blocks(page, pm, g, false, t),
            );
            prev_other = false;
            continue;
        }

        // Unstyled title page: a few short lines, some centered or caps,
        // or with an author line, perhaps under or over a contact block.
        if no == 0
            && let Some(lines) = title_page_lines(page)
        {
            // Title, then perhaps a subtitle ("Novelle", "A Novel") when
            // something else on the page is the author, a line follows it
            // that may be, or the document names an author it is not
            //. Otherwise a second line alone may be the author's
            // name: it is named in the warning instead.
            let mut author = false;
            let mut rest = Vec::new();
            for it in lines {
                if fm.take_author(&it.text, creator) {
                    author = true;
                } else if fm.page_title.is_none() {
                    fm.page_title = Some(it.text.clone());
                } else {
                    rest.push(it.text.clone());
                }
            }
            let other_author = creator.is_some_and(|c| !c.trim().is_empty());
            if fm.subtitle.is_none()
                && (author || other_author || rest.len() >= 2)
                && !rest.is_empty()
            {
                fm.subtitle = Some(rest.remove(0));
                fm.page_subtitle = true;
            }
            fm.dropped
                .extend(rest.into_iter().filter(|l| !contact_line(l)));
            fm.replaced.get_or_insert(at);
            prev_other = false;
            continue;
        }

        // Dedication or epigraph page: up to four set-off lines, and under
        // an epigraph perhaps a dash attribution, as in chapter epigraphs.
        // A page in the Dedication or Epigraph style is one whatever its
        // length or first words: the author named it.
        let chars: usize = page.iter().map(|it| it.text.chars().count()).sum();
        let (lines, attribution) = match page.split_last() {
            Some((last, lines)) if !lines.is_empty() && last.short() => {
                match crate::attribution_text(&last.text) {
                    Some(a) => (lines, Some(a)),
                    None => (page, None),
                }
            }
            _ => (page, None),
        };
        let styled = |p: Purpose, its: &[Item]| {
            its.iter()
                .all(|it| it.p.purpose == Some(p) && !it.has_image())
        };
        let dedication = styled(Purpose::Dedication, page);
        let epigraph = styled(Purpose::Epigraph, lines);
        if dedication
            || epigraph
            || lines.len() <= 4
                && chars <= EPIGRAPH_CHARS
                && lines.iter().all(|it| {
                    !it.has_image() && (it.centered || it.italic || it.p.role == Role::Quote)
                })
        {
            if dedication || !epigraph && is_dedication(&page[0].text) {
                let content = blocks(page, pm, g, false, t);
                push_section(sections, starts, at, SectionKind::Dedication, content);
            } else if let Some(a) = attribution {
                // Set like a chapter epigraph: lines, attribution apart.
                let s = push_section(sections, starts, at, SectionKind::Epigraph, Vec::new());
                sections[s].epigraph = Some(epigraph_of(lines, Some(a)));
            } else {
                let content = blocks(page, pm, g, false, t);
                push_section(sections, starts, at, SectionKind::Epigraph, content);
            }
            prev_other = false;
            continue;
        }

        let content = blocks(page, pm, g, false, t);
        if prev_other && let Some(s) = sections.last_mut() {
            // Consecutive unclassified pages are one section.
            s.blocks.extend(content);
            continue;
        }
        let section = push_section(sections, starts, at, SectionKind::Other, content);
        warnings.push(ImportWarning {
            kind: WarningKind::TextBeforeFirstChapter,
            location: at,
            section: Some(section),
            message: format!(
                "Paragraph {at}: text before the first chapter was kept as an untitled section. \
                 Check whether it belongs in the book."
            ),
            detail: None,
        });
        prev_other = true;
    }
    if let Some(at) = fm.replaced {
        let mut message = "The manuscript's title page was replaced by one generated from the \
                           book's title and author."
            .to_string();
        if fm.page_subtitle
            && let Some(s) = &fm.subtitle
        {
            message += &format!(" “{s}” became the subtitle.");
        }
        if !fm.dropped.is_empty() {
            let lines: Vec<String> = fm.dropped.iter().map(|l| format!("“{l}”")).collect();
            message += &format!(" Not in the book: {}.", lines.join(", "));
        }
        warnings.push(ImportWarning {
            kind: WarningKind::TitlePageReplaced,
            location: at,
            section: None,
            message,
            detail: None,
        });
    }
}

/// Push a front-matter section; returns its index.
fn push_section(
    sections: &mut Vec<Section>,
    starts: &mut Vec<usize>,
    at: usize,
    kind: SectionKind,
    blocks: Vec<Block>,
) -> usize {
    sections.push(Section {
        style: Default::default(),
        kind,
        title: None,
        numbering: Numbering::None,
        epigraph: None,
        blocks,
    });
    starts.push(at);
    sections.len() - 1
}

/// Index ranges of the pages.
fn pages(items: &[Item]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        match out.last_mut() {
            Some(r) if !it.starts_page => r.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// The title lines of an unstyled title page: at most six short lines,
/// some centered or capitalized or an author line. A standard-manuscript
/// contact block (name, address, phone, e-mail, word count: lines that are
/// not centered, at least one of them unmistakable) above or below them is
/// left out, so it never reaches the book. `None` if the page is not one.
fn title_page_lines(page: &[Item]) -> Option<&[Item]> {
    if !page.iter().all(|it| it.short() && !it.text.is_empty()) {
        return None;
    }
    let plain = |it: &&Item| !it.centered && author_line(&it.text).is_none();
    let contact =
        |run: &[Item]| run.len() < page.len() && run.iter().any(|it| contact_line(&it.text));
    let mut lines = page;
    let head = lines.iter().take_while(plain).count();
    if contact(&lines[..head]) {
        lines = &lines[head..];
    }
    let tail = lines.iter().rev().take_while(plain).count();
    if contact(&lines[lines.len() - tail..]) {
        lines = &lines[..lines.len() - tail];
    }
    (!lines.is_empty()
        && lines.len() <= 6
        && lines
            .iter()
            .any(|it| it.centered || it.caps || author_line(&it.text).is_some()))
    .then_some(lines)
}

/// A line only a contact block has: an e-mail or web address, a phone
/// number, a word count ("approx. 4,000 words", "ca. 1.800 Wörter").
fn contact_line(text: &str) -> bool {
    let l = text.to_lowercase();
    let digits = l.chars().filter(char::is_ascii_digit).count();
    let mail = l.split_whitespace().any(|w| {
        w.split_once('@')
            .is_some_and(|(user, host)| !user.is_empty() && host.contains('.'))
    });
    let web = vocab::WEB_MARKERS.iter().any(|w| l.contains(w));
    // Mostly digits: "(207) 555-0142", "+49 30 1234567", "Tel. 030 12 34 56".
    let visible = l.chars().filter(|c| !c.is_whitespace()).count();
    let phone = digits >= 7 && digits * 10 >= visible * 6;
    let count = digits > 0 && vocab::COUNT_WORDS.iter().any(|w| l.contains(w));
    mail || web || phone || count
}

/// "by Jane Doe" / "von Lena Brandt" → the name. A name is a few
/// capitalized words, so "By the time …" or "Von Anfang an." is not one.
fn author_line(text: &str) -> Option<String> {
    let rest = vocab::AUTHOR_PREFIXES.iter().find_map(|p| {
        text.get(..p.len())
            .filter(|h| h.eq_ignore_ascii_case(p))
            .map(|_| &text[p.len()..])
    })?;
    let name = rest.trim();
    let words: Vec<&str> = name.split_whitespace().collect();
    let ok = !words.is_empty()
        && words.len() <= 8
        && name.chars().count() <= 60
        && words.iter().all(|w| {
            w.starts_with(|c: char| c.is_uppercase())
                || vocab::AUTHOR_PARTICLES.contains(&w.to_lowercase().as_str())
        })
        && !name.ends_with([',', ';', ':', '!', '?']);
    ok.then(|| name.to_string())
}

fn is_copyright(text: &str) -> bool {
    let l = text.to_lowercase();
    l.contains('©')
        || vocab::COPYRIGHT_MARKERS
            .iter()
            .any(|m| l.contains(m) && (*m != vocab::COPYRIGHT_LEAD || l.starts_with(m)))
}

// ---------------------------------------------------------- body blocks

/// Count and first paragraph of something reported once in aggregate.
#[derive(Default)]
struct Tally {
    count: usize,
    first: usize,
}

impl Tally {
    fn note(&mut self, para: usize) {
        if self.count == 0 {
            self.first = para;
        }
        self.count += 1;
    }
}

/// Scene breaks made from empty paragraphs in one section.
struct BlankBreaks {
    section: usize,
    /// Where the section is, for the author: "under the heading “…”".
    place: String,
    tally: Tally,
    /// The first words after the first break.
    words: String,
}

#[derive(Default)]
struct Tallies {
    line_breaks: Tally,
    page_breaks: Tally,
    /// Number and place of the section being built.
    section: (usize, String),
    blank_breaks: Vec<BlankBreaks>,
}

impl Tallies {
    fn start_section(&mut self, place: String) {
        self.section = (self.section.0 + 1, place);
    }

    /// A scene break made from empty paragraphs before `text`.
    fn blank_break(&mut self, para: usize, text: &str) {
        let (section, place) = &self.section;
        if let Some(b) = self.blank_breaks.last_mut()
            && b.section == *section
        {
            b.tally.note(para);
            return;
        }
        let mut tally = Tally::default();
        tally.note(para);
        self.blank_breaks.push(BlankBreaks {
            section: *section,
            place: place.clone(),
            tally,
            words: opening(text),
        });
    }

    fn report(&self, warnings: &mut Vec<ImportWarning>) {
        let mut push = |kind, t: &Tally, msg: String| {
            if t.count > 0 {
                warnings.push(ImportWarning {
                    kind,
                    location: t.first,
                    section: None,
                    message: msg,
                    detail: None,
                });
            }
        };
        let t = &self.line_breaks;
        push(
            WarningKind::LineBreaksJoined,
            t,
            format!(
                "{} manual line break(s) inside paragraphs were replaced by spaces (first in \
                 paragraph {}). Where a break is meant, use separate paragraphs or the Verse style.",
                t.count, t.first
            ),
        );
        let t = &self.page_breaks;
        push(
            WarningKind::PageBreaksIgnored,
            t,
            format!(
                "{} page break(s) inside chapters were ignored (first before paragraph {}); \
                 the theme decides where pages break.",
                t.count, t.first
            ),
        );
        // One warning per section, so each chapter is named.
        for b in &self.blank_breaks {
            let place = if b.place.is_empty() {
                String::new()
            } else {
                format!(" {}", b.place)
            };
            push(
                WarningKind::SceneBreaksFromBlankLines,
                &b.tally,
                format!(
                    "{} scene break(s){place} were made from empty lines, the first before “{}”. \
                     Check them, or mark scene breaks with *** in the manuscript.",
                    b.tally.count, b.words
                ),
            );
        }
    }
}

/// The first words of a paragraph, to quote in a warning.
fn opening(text: &str) -> String {
    const WORDS: usize = 6;
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= WORDS {
        words.join(" ")
    } else {
        format!("{}…", words[..WORDS].join(" "))
    }
}

/// How a run of paragraphs is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Quote,
    Verse,
    List(bool),
    /// A table; never grouped with anything.
    Table,
}

fn kind(it: &Item) -> Kind {
    if it.p.table.is_some() {
        return Kind::Table;
    }
    if let Some(ordered) = it.p.list {
        return Kind::List(ordered);
    }
    if it.p.role == Role::Verse || (it.p.role == Role::Body && it.verse_like()) {
        return Kind::Verse;
    }
    let indented = it.p.ind_left >= QUOTE_INDENT && it.p.ind_right >= QUOTE_INDENT;
    if it.p.role == Role::Quote || (it.p.role == Role::Body && indented && !it.centered) {
        return Kind::Quote;
    }
    Kind::Plain
}

/// The epigraph at the start of a section and the number of paragraphs it
/// takes: up to four paragraphs set off from the text (Quote or Epigraph
/// style, indented on both sides, centered or italic), then one that starts
/// with a dash, the attribution (plain text). Italic that covers a whole
/// paragraph is the manuscript's epigraph style and dropped (the theme sets
/// epigraphs); paragraphs become lines. A set-off opening without a dash
/// line (a letter, a dream in italics) stays in the text.
///
/// Paragraphs in the Epigraph (Motto) style are the author's word: any
/// number of them, of any length, are the epigraph, with the dash line as
/// its attribution or, without one, none.
fn section_epigraph(items: &[Item], marks: &[Mark]) -> Option<(Epigraph, usize)> {
    let mut chars = 0;
    // How many paragraphs from the start are in the Epigraph style.
    let mut styled = 0;
    for (k, it) in items.iter().enumerate() {
        let indented = it.p.ind_left >= QUOTE_INDENT && it.p.ind_right >= QUOTE_INDENT;
        let set_off = it.p.role == Role::Quote || indented || it.centered || it.italic;
        chars += it.text.chars().count();
        if styled == k && it.p.purpose == Some(Purpose::Epigraph) {
            styled += 1;
        }
        let guessed = styled <= k;
        if marks[k] != Mark::Body
            || (k > 0 && it.starts_page)
            || !set_off
            || it.p.list.is_some()
            || it.has_image()
            || it.is_scene_break()
            || guessed && (k >= EPIGRAPH_PARAS || chars > EPIGRAPH_CHARS)
        {
            return (styled > 0 && styled == k).then(|| (epigraph_of(&items[..k], None), k));
        }
        if k > 0
            && let Some(attribution) = crate::attribution_text(&it.text)
        {
            return Some((epigraph_of(&items[..k], Some(attribution)), k + 1));
        }
    }
    (styled > 0 && styled == items.len()).then(|| (epigraph_of(items, None), items.len()))
}

/// An epigraph from its paragraphs (one line each) and attribution.
/// Italic that covers a whole paragraph is the manuscript's epigraph style
/// and dropped.
fn epigraph_of(lines: &[Item], attribution: Option<&str>) -> Epigraph {
    let mut text = Vec::new();
    for (n, q) in lines.iter().enumerate() {
        if n > 0 {
            text.push(Inline::LineBreak);
        }
        let mut segs = q.p.segs.clone();
        if q.italic {
            segs.iter_mut().for_each(|s| s.fmt.em = false);
        }
        inline::normalize(&mut segs);
        text.extend(inline::to_inlines(&segs));
    }
    Epigraph {
        text,
        attribution: attribution.map(str::to_string),
    }
}

/// Body blocks of a section. `in_body`: page breaks are reported as
/// ignored (front-matter pages break by design).
fn blocks(items: &[Item], marks: &[Mark], g: usize, in_body: bool, t: &mut Tallies) -> Vec<Block> {
    let mut out = Vec::new();
    // A scene break to put before the next block: `Some(None)` from an
    // ornament, `Some(Some(k))` from empty paragraphs before `items[k]`.
    let mut pending: Option<Option<usize>> = None;
    let mut k = 0;
    while k < items.len() {
        let it = &items[k];
        if in_body && it.starts_page && k > 0 {
            t.page_breaks.note(it.p.index);
        }
        if it.blanks > g {
            pending.get_or_insert(Some(k));
        }
        if it.is_scene_break() {
            pending = Some(None);
            k += 1;
            continue;
        }
        // Paragraphs `k..j` that continue a group of `kd`.
        let group = |kd: Kind, strict: bool| {
            let mut j = k + 1;
            while let Some(n) = items.get(j) {
                if marks[j] != Mark::Body
                    || n.starts_page
                    || n.is_scene_break()
                    || kind(n) != kd
                    || (strict && n.blanks > g)
                {
                    break;
                }
                j += 1;
            }
            j
        };
        let (new, next) = match (marks[k], kind(it)) {
            (Mark::Sub(level), _) => (
                vec![Block::heading(level, joined(&it.p.segs, it.p.index, None))],
                k + 1,
            ),
            (_, Kind::Verse) => {
                let j = group(Kind::Verse, false);
                (vec![verse(&items[k..j], g)], j)
            }
            (_, Kind::Quote) => {
                let j = group(Kind::Quote, true);
                let mut inner = Vec::new();
                for q in &items[k..j] {
                    if q.verse_like() {
                        inner.push(verse(std::slice::from_ref(q), g));
                    } else {
                        inner.extend(para_blocks(&q.p, Some(&mut t.line_breaks)));
                    }
                }
                (vec![Block::block_quote(inner)], j)
            }
            (_, Kind::List(ordered)) => {
                let j = group(Kind::List(ordered), true);
                let items = items[k..j]
                    .iter()
                    .map(|li| para_blocks(&li.p, Some(&mut t.line_breaks)))
                    .collect();
                (vec![Block::list(ordered, items)], j)
            }
            (_, Kind::Table) => match &it.p.table {
                Some(data) => (vec![table_block(data)], k + 1),
                None => (Vec::new(), k + 1),
            },
            (_, Kind::Plain) => {
                let mut b = para_blocks(&it.p, Some(&mut t.line_breaks));
                let mut next = k + 1;
                // A caption right after an image.
                if let Some(Block::Image { caption, .. }) = b.last_mut()
                    && let Some(c) = items.get(k + 1)
                    && c.p.role == Role::Caption
                    && !c.starts_page
                {
                    *caption = Some(joined(&c.p.segs, c.p.index, None));
                    next += 1;
                }
                (b, next)
            }
        };
        for b in new {
            if let Some(p) = pending.take()
                && !out.is_empty()
            {
                out.push(Block::scene_break());
                if let Some(at) = p {
                    // A table has no text of its own: quote its first cell.
                    let lead = items[at].p.table.as_ref().and_then(|data| {
                        data.rows
                            .iter()
                            .flat_map(|r| &r.cells)
                            .map(|c| inline::plain_text(&c.segs).trim().to_string())
                            .find(|s| !s.is_empty())
                    });
                    t.blank_break(
                        items[at].p.index,
                        lead.as_deref().unwrap_or(&items[at].text),
                    );
                }
            }
            out.push(b);
        }
        k = next;
    }
    out
}

/// A table's rows as lines of plain text, cells joined by " · "; a row with
/// no text is left out.
fn table_lines(data: &TableData) -> Vec<String> {
    data.rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .map(|c| inline::plain_text(&c.segs).trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// The column alignment a cell's paragraph alignment means.
fn cell_align(a: Align) -> ColumnAlign {
    match a {
        Align::Center => ColumnAlign::Center,
        Align::End => ColumnAlign::Right,
        Align::Start | Align::Justify => ColumnAlign::Left,
    }
}

/// The table block of a table the story read: header rows as marked (and
/// grown over the spans that start in them), the columns aligned as most of
/// their body cells are, cells as inlines with the spans Word wrote. A
/// cell over several columns keeps its own alignment when it differs from
/// its first column's.
fn table_block(data: &TableData) -> Block {
    let columns = data.columns.max(1);
    // The body rows decide; a table of header rows only counts them all.
    let body = if data.header_rows < data.rows.len() {
        &data.rows[data.header_rows..]
    } else {
        &data.rows[..]
    };
    let aligns: Vec<ColumnAlign> = (0..columns)
        .map(|c| {
            // Left (and justified), center, right: the most common among the
            // single-column cells with text that start in the column; a tie
            // goes left.
            let mut n = [0usize; 3];
            for cell in body
                .iter()
                .flat_map(|r| &r.cells)
                .filter(|cell| cell.col == c && cell.colspan == 1)
            {
                if !cell.segs.iter().any(visible) {
                    continue;
                }
                n[match cell_align(cell.align) {
                    ColumnAlign::Left => 0,
                    ColumnAlign::Center => 1,
                    ColumnAlign::Right => 2,
                }] += 1;
            }
            if n[2] > n[0] && n[2] > n[1] {
                ColumnAlign::Right
            } else if n[1] > n[0] && n[1] > n[2] {
                ColumnAlign::Center
            } else {
                ColumnAlign::Left
            }
        })
        .collect();
    let rows = data
        .rows
        .iter()
        .map(|r| {
            r.cells
                .iter()
                .map(|cell| {
                    let mut segs = cell.segs.clone();
                    let has_text = segs.iter().any(visible);
                    inline::normalize(&mut segs);
                    let mut out = TableCell::new(inline::to_inlines(&segs));
                    out.colspan = u32::try_from(cell.colspan).unwrap_or(1);
                    out.rowspan = u32::try_from(cell.rowspan).unwrap_or(1);
                    let own = cell_align(cell.align);
                    if cell.colspan > 1
                        && has_text
                        && aligns.get(cell.col).is_some_and(|&first| first != own)
                    {
                        out.align = Some(own);
                    }
                    out
                })
                .collect()
        })
        .collect();
    Block::Table(Table::new(
        aligns.into_iter().map(TableColumn::new).collect(),
        u32::try_from(data.header_rows).unwrap_or(0),
        rows,
    ))
}

/// A footnote or endnote paragraph's blocks.
pub(super) fn note_blocks(p: &Para) -> Vec<Block> {
    para_blocks(p, None)
}

/// A paragraph's blocks: text with line breaks joined, split at images.
fn para_blocks(p: &Para, line_breaks: Option<&mut Tally>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Vec<Seg> = Vec::new();
    let mut broke = false;
    let mut flush = |cur: &mut Vec<Seg>, out: &mut Vec<Block>| {
        if cur.iter().any(visible) {
            broke |= inner_line_break(cur);
            out.push(Block::paragraph(joined(cur, p.index, None)));
        }
        cur.clear();
    };
    // A picture the text wraps round in Word sits beside the paragraph it
    // is anchored in, which stays whole: it goes ahead of it.
    let mut beside = Vec::new();
    for s in &p.segs {
        if let Piece::Image {
            asset, alt, place, ..
        } = &s.piece
        {
            let mut image = Block::image(asset.clone(), alt.clone(), None);
            if let Some(width) = place.width {
                image = image.with_width(width);
            }
            image = image.with_placement(place.placement).with_side(place.side);
            if place.placement == ImagePlacement::Beside {
                beside.push(image);
            } else {
                flush(&mut cur, &mut out);
                out.push(image);
            }
        } else {
            cur.push(s.clone());
        }
    }
    flush(&mut cur, &mut out);
    if broke && let Some(t) = line_breaks {
        t.note(p.index);
    }
    beside.append(&mut out);
    beside
}

fn visible(s: &Seg) -> bool {
    match &s.piece {
        Piece::Text(t) => !t.trim().is_empty(),
        Piece::Note(_) | Piece::Image { .. } => true,
        Piece::LineBreak => false,
    }
}

/// Whether a line break separates visible content (a break at the start or
/// end of a paragraph is noise).
fn inner_line_break(segs: &[Seg]) -> bool {
    match (
        segs.iter().position(visible),
        segs.iter().rposition(visible),
    ) {
        (Some(a), Some(b)) => segs[a..b]
            .iter()
            .any(|s| matches!(s.piece, Piece::LineBreak)),
        _ => false,
    }
}

/// Inlines with line breaks replaced by spaces.
fn joined(segs: &[Seg], para: usize, t: Option<&mut Tally>) -> Vec<Inline> {
    if inner_line_break(segs)
        && let Some(t) = t
    {
        t.note(para);
    }
    let mut segs = segs.to_vec();
    for s in &mut segs {
        if matches!(s.piece, Piece::LineBreak) {
            s.piece = Piece::Text(" ".into());
        }
    }
    inline::normalize(&mut segs);
    inline::to_inlines(&segs)
}

fn split_lines(segs: &[Seg]) -> Vec<Vec<Seg>> {
    let mut out = vec![Vec::new()];
    for s in segs {
        if matches!(s.piece, Piece::LineBreak) {
            out.push(Vec::new());
        } else if let Some(l) = out.last_mut() {
            l.push(s.clone());
        }
    }
    out
}

/// A verse block from consecutive verse paragraphs. A paragraph with line
/// breaks is a stanza; single-line paragraphs form stanzas separated by
/// empty paragraphs or by space before or after that is clearly more than
/// the verse's usual spacing ("Stanza Start" styles). A paragraph's left
/// indent beyond its style's, less that of the least-indented paragraph (a
/// poem indented as a whole is flush), gives its lines their indent level.
fn verse(items: &[Item], g: usize) -> Block {
    // Where the paragraph's first line starts, less where its style would
    // put it. A one-line paragraph also counts its first-line indent (what
    // Word makes of a Tab typed at the start of a line); in a stanza the
    // first line's indent is a different thing and is not read.
    let beyond = |it: &Item| {
        let single = split_lines(&it.p.segs).len() == 1;
        let (first, style_first) = if single {
            (it.p.ind_first, it.p.ind_style_first)
        } else {
            (0, 0)
        };
        let start = it.p.ind_left.saturating_add(first);
        let style = it.p.ind_style_left.saturating_add(style_first);
        start.saturating_sub(style).max(0)
    };
    let least = items.iter().map(beyond).min().unwrap_or(0);
    let level = |it: &Item| {
        let steps = beyond(it)
            .saturating_sub(least)
            .saturating_add(VERSE_INDENT_STEP / 2)
            / VERSE_INDENT_STEP;
        u8::try_from(steps).map_or(MAX_VERSE_INDENT, |l| l.min(MAX_VERSE_INDENT))
    };
    // Space between paragraphs k - 1 and k; Word adds the two.
    let space = |k: usize| {
        items[k - 1]
            .p
            .space_after
            .saturating_add(items[k].p.space_before)
    };
    let mut counts: Vec<(i64, usize)> = Vec::new();
    for k in 1..items.len() {
        match counts.iter_mut().find(|(s, _)| *s == space(k)) {
            Some((_, n)) => *n += 1,
            None => counts.push((space(k), 1)),
        }
    }
    let usual = counts
        .iter()
        .max_by_key(|&&(s, n)| (n, std::cmp::Reverse(s)))
        .map_or(0, |&(s, _)| s);
    let mut lines: Vec<Vec<Inline>> = Vec::new();
    let mut indents: Vec<u8> = Vec::new();
    let mut prev_multi = false;
    for (k, it) in items.iter().enumerate() {
        let mut stanza: Vec<Vec<Inline>> = Vec::new();
        for l in split_lines(&it.p.segs) {
            let content = if l.iter().any(visible) {
                inline::to_inlines(&l)
            } else {
                Vec::new()
            };
            // One empty line between stanzas, none at the ends.
            if content.is_empty() && stanza.last().is_none_or(Vec::is_empty) {
                continue;
            }
            stanza.push(content);
        }
        while stanza.last().is_some_and(Vec::is_empty) {
            stanza.pop();
        }
        let multi = stanza.len() > 1;
        let gap = k > 0 && (it.blanks > g || space(k) >= usual.saturating_add(STANZA_SPACE));
        if k > 0 && (multi || prev_multi || gap) && !stanza.is_empty() {
            lines.push(Vec::new());
            indents.push(0);
        }
        indents.extend(
            stanza
                .iter()
                .map(|l| if l.is_empty() { 0 } else { level(it) }),
        );
        lines.extend(stanza);
        prev_multi = multi;
    }
    Block::verse(lines).with_indents(indents)
}

// ------------------------------------------------------------- metadata

fn metadata(
    items: &[Item],
    fm: Front,
    doc_lang: Option<&str>,
    core: &Core,
    warnings: &mut Vec<ImportWarning>,
) -> Metadata {
    let nonempty = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let typed = nonempty(&fm.title)
        .or_else(|| nonempty(&fm.heading_title))
        .or_else(|| nonempty(&fm.page_title));
    let (title, caps_note) = title_case(typed, nonempty(&core.title));
    if let Some(note) = caps_note
        && let Some(w) = warnings
            .iter_mut()
            .find(|w| w.kind == WarningKind::TitlePageReplaced)
    {
        w.message = format!("{} {note}", w.message);
    }
    if title.is_empty() {
        warnings.push(ImportWarning {
            kind: WarningKind::NoTitle,
            location: 0,
            section: None,
            message: "No book title was found (no Title style, title page or document title). \
                      Enter it in the book's details."
                .into(),
            detail: None,
        });
    }
    // Producers fill dc:creator with placeholders (calibre: "Unknown").
    let creator = nonempty(&core.creator)
        .filter(|c| !vocab::CREATOR_PLACEHOLDERS.contains(&c.to_lowercase().as_str()));
    let authors = nonempty(&fm.author).or(creator).into_iter().collect();
    Metadata {
        title,
        subtitle: nonempty(&fm.subtitle),
        authors,
        lang: choose_lang(
            crate::lang::detect(items.iter().map(|it| it.text.as_str())),
            doc_lang,
            core.language.as_deref(),
        ),
        copyright: fm.copyright,
        ..Metadata::default()
    }
}

/// The book's title from the manuscript's title lines (`typed`), else the
/// document properties, and a note for the title-page warning when the
/// typed title is in capitals. Capitals on a title page are a
/// display choice (the theme's now), but the metadata title is what store
/// listings show: when the properties spell the same words in normal
/// case, that spelling is the author's own and wins. Otherwise the
/// capitals stay, because recasing them is a guess ("NASA", "McKAY").
fn title_case(typed: Option<String>, core: Option<String>) -> (String, Option<String>) {
    let in_caps = |s: &str| {
        let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
        letters.len() >= 2 && letters.iter().all(|c| !c.is_lowercase())
    };
    match (typed, core) {
        (Some(t), Some(c)) if in_caps(&t) && !in_caps(&c) && same_words(&t, &c) => {
            let note = format!(
                "The title is “{c}”, as in the document properties, not in the capitals typed \
                 on the title page."
            );
            (c, Some(note))
        }
        (Some(t), _) if in_caps(&t) => {
            let note = format!(
                "The title “{t}” is in capitals as typed; if it is not meant so, correct it in \
                 the book's details."
            );
            (t, Some(note))
        }
        (Some(t), _) => (t, None),
        (None, c) => (c.unwrap_or_default(), None),
    }
}

/// The same words, apart from case and spacing.
fn same_words(a: &str, b: &str) -> bool {
    let words = |s: &str| {
        s.to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    words(a) == words(b)
}

/// The detected language, keeping a declared tag with the same primary
/// subtag (so `de-AT` survives); without detection the declared tag.
fn choose_lang(detected: Option<&str>, doc: Option<&str>, core: Option<&str>) -> Lang {
    let declared: Vec<Lang> = [doc, core]
        .into_iter()
        .flatten()
        .map(|t| t.trim().replace('_', "-"))
        .filter(|t| !t.is_empty())
        .map(Lang)
        .collect();
    match detected {
        Some(p) => declared
            .into_iter()
            .find(|l| l.primary() == p)
            .unwrap_or_else(|| Lang(p.into())),
        None => declared.into_iter().next().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_lines() {
        assert_eq!(
            author_line("by Elizabeth Bennet").as_deref(),
            Some("Elizabeth Bennet")
        );
        assert_eq!(
            author_line("von Franz Kafka").as_deref(),
            Some("Franz Kafka")
        );
        assert_eq!(
            author_line("By Jane Doe and John Roe").as_deref(),
            Some("Jane Doe and John Roe")
        );
        assert_eq!(author_line("Mr. Bennet replied that he had not."), None);
        assert_eq!(author_line("So sind die Leute in meiner Gegend."), None);
        assert_eq!(author_line("by"), None);
    }

    #[test]
    fn datelines() {
        for yes in [
            "Halvard Street Station — 11:42 p.m.",
            "Carrow Junction — 12:22 a.m.",
            "23.15 Uhr",
            "Kurz nach 8 Uhr",
            "Berlin, 9. November 1989",
            "9.11.1989",
            "Leipzig, 14. 10. 2026",
            "3/14/79",
            "London, 1888",
            "1979, Gorran Holm",
            "Longbourn Street—1813",
            "15 November 1978",
        ] {
            assert!(dateline(yes), "{yes}");
        }
        for no in [
            "From the folder",
            "The Summer of 1813",
            "Chapter Two",
            "First Impressions.",
            "3 Things",
            "Teil 2",
            "Mai",
            "1914–1918",
        ] {
            assert!(!dateline(no), "{no}");
        }
    }

    #[test]
    fn contact_lines() {
        for yes in [
            "nell.ashcombe@example.com",
            "E-Mail: post@wendt-uhren.de",
            "(207) 555-0142",
            "Tel. +49 30 1234567",
            "Jane Austen approx. 4,000 words",
            "ca. 1.800 Wörter",
            "www.example.org",
        ] {
            assert!(contact_line(yes), "{yes}");
        }
        for no in [
            "218 Alder Street",
            "Portland, ME 04101",
            "NORTHANGER ABBEY",
            "@home",
        ] {
            assert!(!contact_line(no), "{no}");
        }
    }
}
