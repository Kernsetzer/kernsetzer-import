//! Streaming parser for a WordprocessingML story (`document.xml`,
//! `footnotes.xml`, `endnotes.xml`): a flat list of paragraphs with
//! resolved formatting, page breaks, notes and images. A table of the body
//! is one paragraph that carries its rows and cells (`TableData`). Structure
//! (chapters, scene breaks) is decided later in `structure`.
//!
//! What is dropped here, deliberately: deleted text of tracked changes
//! (insertions are kept, i.e. changes are accepted), field codes (field
//! results are kept), hidden text, comments, paragraph-mark formatting,
//! text boxes (with a warning), the `mc:Fallback` branch of alternate
//! content.

use super::media::Media;
use super::package::{Package, Rel};
use super::pictures::{self, Layout, Page, Place, Sect, Wrap};
use super::styles::{
    Align, Numbering, ParaProps, ParaStyle, Purpose, Role, RunProps, Styles, VertAlign,
};
use super::table::{self, Merge, RawCell, RawRow};
use super::xml::{attr, int_attr, on_off, resolve_ref, val};
use crate::{ImportWarning, WarningKind};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use setzer_model::Block;
use std::collections::HashMap;

/// The largest indent or spacing read from a file, in twips (22 inches).
const MAX_TWIPS: i64 = 31_680;

/// Inline formatting that maps to the model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Fmt {
    pub em: bool,
    pub strong: bool,
    pub small_caps: bool,
    pub sup: bool,
    pub sub: bool,
}

impl Fmt {
    pub fn common(self, o: Fmt) -> Fmt {
        Fmt {
            em: self.em && o.em,
            strong: self.strong && o.strong,
            small_caps: self.small_caps && o.small_caps,
            sup: self.sup && o.sup,
            sub: self.sub && o.sub,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Piece {
    Text(String),
    LineBreak,
    Note(Vec<Block>),
    /// A picture; `place` is filled in once the whole story is read
    /// (`pictures::resolve`).
    Image {
        asset: String,
        alt: String,
        layout: Layout,
        place: Place,
    },
}

/// A run of content with one formatting.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Seg {
    pub fmt: Fmt,
    pub link: Option<String>,
    /// Displayed in capitals (`w:caps`); only used to recognize headings.
    pub caps: bool,
    /// Font size in half-points, if known; only used to recognize headings.
    pub size: Option<u16>,
    pub piece: Piece,
}

/// A segment of `piece` with no formatting.
pub(super) fn plain_seg(piece: Piece) -> Seg {
    Seg {
        fmt: Fmt::default(),
        link: None,
        caps: false,
        size: None,
        piece,
    }
}

impl Seg {
    pub fn text(&self) -> Option<&str> {
        match &self.piece {
            Piece::Text(t) => Some(t),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct Para {
    /// 1-based paragraph number in the story, for warnings.
    pub index: usize,
    /// Footnote or endnote id (notes stories only).
    pub note: Option<(NoteKind, i64)>,
    /// Paragraph style id (`None`: the default style).
    pub style: Option<String>,
    pub role: Role,
    /// The element the paragraph's style is named for (Epigraph,
    /// Dedication), if any.
    pub purpose: Option<Purpose>,
    pub align: Align,
    pub ind_left: i64,
    pub ind_right: i64,
    /// The left indent the paragraph's style gives it (twips): what
    /// `ind_left` is without the paragraph's own formatting.
    pub ind_style_left: i64,
    /// The first line's indent relative to `ind_left` (twips), and the
    /// style's own.
    pub ind_first: i64,
    pub ind_style_first: i64,
    /// Space before and after in twips.
    pub space_before: i64,
    pub space_after: i64,
    pub page_break_before: bool,
    /// A section break (new page) follows this paragraph.
    pub break_after: bool,
    /// `Some(ordered)` for list items.
    pub list: Option<bool>,
    pub in_table: bool,
    /// Set on the one paragraph that stands for a whole table (its
    /// `in_table` is true as well); `segs` is empty then.
    pub table: Option<TableData>,
    /// Index of the section the paragraph is in, for `pictures::resolve`.
    pub section: usize,
    /// Part of a generated table of contents.
    pub toc: bool,
    pub segs: Vec<Seg>,
}

impl Para {
    /// Whether the paragraph shows anything (text, image, note mark).
    pub fn is_visible(&self) -> bool {
        self.table.is_some()
            || self.segs.iter().any(|s| match &s.piece {
                Piece::Text(t) => !t.trim().is_empty(),
                Piece::LineBreak => false,
                Piece::Note(_) | Piece::Image { .. } => true,
            })
    }
}

impl Para {
    /// Like `is_visible`, but zero-width spaces are not text: what the
    /// paragraph will still show once `inline::normalize` has cleaned it.
    fn shows_content(&self) -> bool {
        self.table.is_some()
            || self.segs.iter().any(|s| match &s.piece {
                Piece::Text(t) => t.chars().any(|c| {
                    !c.is_whitespace() && !c.is_control() && !matches!(c, '\u{200B}' | '\u{FEFF}')
                }),
                Piece::LineBreak => false,
                Piece::Note(_) | Piece::Image { .. } => true,
            })
    }
}

/// A table the story read: the outermost `w:tbl` of the
/// body, rows of the cells that start in them, laid on Word's grid
/// (`table::resolve`).
#[derive(Debug, Clone, Default)]
pub(super) struct TableData {
    pub rows: Vec<DataRow>,
    /// The width of the grid.
    pub columns: usize,
    /// The leading rows marked `w:tblHeader`, grown to the end of a span
    /// that starts in them.
    pub header_rows: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DataRow {
    pub cells: Vec<DataCell>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DataCell {
    /// The segments of the cell's visible paragraphs, a line break between
    /// two paragraphs. Nothing else of a paragraph is kept: no indent, no
    /// spacing, no style.
    pub segs: Vec<Seg>,
    /// The alignment of the cell's first visible paragraph: the only
    /// paragraph property that counts, and it counts for the column.
    pub align: Align,
    /// The grid column the cell starts in, and how many columns and rows it
    /// reaches (`w:gridSpan`, `w:vMerge`).
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum NoteKind {
    Foot,
    End,
}

/// Everything the parser reads from or reports to.
pub(super) struct Env<'a, 'p> {
    pub pkg: &'a mut Package<'p>,
    pub styles: &'a Styles,
    pub numbering: &'a Numbering,
    pub rels: &'a HashMap<String, Rel>,
    pub notes: &'a HashMap<(NoteKind, i64), Vec<Block>>,
    pub media: &'a mut Media,
    pub warnings: &'a mut Vec<ImportWarning>,
}

#[derive(Debug, Default)]
struct RunState {
    style: Option<String>,
    direct: RunProps,
    /// `w:customMarkFollows`: the next text is a custom footnote mark.
    skip_text: bool,
}

#[derive(Debug, Default)]
struct Field {
    instr: String,
    in_result: bool,
    toc: bool,
    link: Option<String>,
}

/// A row of the outermost table being read.
#[derive(Debug, Default)]
struct Row {
    /// Index in `Parser::out` of the row's first paragraph and of each
    /// cell's.
    start: usize,
    cells: Vec<usize>,
    /// A header row (`w:tblHeader`), repeated on every page in Word.
    header: bool,
    /// `w:gridBefore`, `w:gridAfter`.
    before: usize,
    after: usize,
    /// What each cell's `w:tcPr` says, parallel to `cells`.
    props: Vec<CellProps>,
}

/// What a cell's `w:tcPr` says that the model can use.
#[derive(Debug, Default, Clone, Copy)]
struct CellProps {
    span: usize,
    v_merge: Merge,
    h_merge: Merge,
    /// `w:textDirection` other than left to right, top to bottom.
    rotated: bool,
}

#[derive(Debug, Default)]
struct Drawing {
    rel: Option<String>,
    linked: bool,
    alt: String,
    text_box: bool,
    layout: Layout,
    /// Inside `wp:positionH` (true) or `wp:positionV` (false).
    horizontal: Option<bool>,
    /// The text of the next `wp:align` / `wp:posOffset` is wanted.
    want: Want,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Want {
    #[default]
    Nothing,
    Align,
    Offset,
}

struct Parser<'e, 'a, 'p> {
    env: &'e mut Env<'a, 'p>,
    out: Vec<Para>,
    para: Option<Para>,
    para_style_id: Option<String>,
    para_direct: ParaProps,
    para_style: ParaStyle,
    resolved: bool,
    run: Option<RunState>,
    in_ppr: bool,
    in_rpr: bool,
    in_sectpr: bool,
    /// The section properties being read, and the text block of each
    /// section read so far.
    sect: Sect,
    pages: Vec<Page>,
    in_t: bool,
    in_instr: bool,
    /// Depth of an ignored subtree (0 = not skipping).
    skip: usize,
    drawing: Option<Drawing>,
    hyperlinks: Vec<Option<String>>,
    fields: Vec<Field>,
    table_depth: usize,
    /// Index in `out` of the outermost table's first paragraph.
    table_start: usize,
    /// The rows of the outermost table read so far.
    rows: Vec<Row>,
    /// A table inside a cell of the outermost table was seen.
    table_nested: bool,
    row: Option<Row>,
    /// The row of a table inside a cell being read.
    nested_row: Option<Row>,
    in_trpr: bool,
    in_tcpr: bool,
    note: Option<(NoteKind, i64)>,
    count: usize,
    tracked_changes: bool,
    char_styles: HashMap<String, RunProps>,
}

/// Parse a story into paragraphs. Never fails: a damaged part yields the
/// paragraphs read so far and a warning.
pub(super) fn parse(xml: &str, env: &mut Env) -> Vec<Para> {
    let para_style = env.styles.para(None);
    let mut p = Parser {
        env,
        out: Vec::new(),
        para: None,
        para_style_id: None,
        para_direct: ParaProps::default(),
        para_style,
        resolved: false,
        run: None,
        in_ppr: false,
        in_rpr: false,
        in_sectpr: false,
        sect: Sect::default(),
        pages: Vec::new(),
        in_t: false,
        in_instr: false,
        skip: 0,
        drawing: None,
        hyperlinks: Vec::new(),
        fields: Vec::new(),
        table_depth: 0,
        table_start: 0,
        rows: Vec::new(),
        table_nested: false,
        row: None,
        nested_row: None,
        in_trpr: false,
        in_tcpr: false,
        note: None,
        count: 0,
        tracked_changes: false,
        char_styles: HashMap::new(),
    };
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => p.start(&e, false),
            Ok(Event::Empty(e)) => p.start(&e, true),
            Ok(Event::End(e)) => p.end(e.local_name().into_inner()),
            Ok(Event::Text(t)) => p.text(&t.xml10_content()),
            Ok(Event::GeneralRef(r)) => {
                if let Some(c) = resolve_ref(&r) {
                    p.text(c.encode_utf8(&mut [0; 4]));
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                p.env.warnings.push(ImportWarning {
                    kind: WarningKind::DamagedFile,
                    location: p.count,
                    section: None,
                    message: format!(
                        "The document is damaged after paragraph {} ({e}); the rest could not be read.",
                        p.count
                    ),
    detail: None,
                });
                break;
            }
            _ => {}
        }
    }
    p.finish_para();
    if p.table_depth > 0 {
        // A file that ends inside a table keeps the table it has.
        p.finish_row();
        p.finish_table();
    }
    pictures::resolve(&mut p.out, &p.pages, p.env.warnings);
    if p.tracked_changes {
        p.env.warnings.push(ImportWarning {
            kind: WarningKind::TrackedChangesAccepted,
            location: 0,
            section: None,
            message: "The manuscript contains tracked changes. They were accepted; \
                      check the text if some changes were meant to be rejected."
                .into(),
            detail: None,
        });
    }
    p.out
}

impl Parser<'_, '_, '_> {
    fn start(&mut self, e: &BytesStart, empty: bool) {
        if self.skip > 0 {
            if !empty {
                self.skip += 1;
            }
            return;
        }
        let name = e.local_name().into_inner();
        let skip = |p: &mut Self| {
            if !empty {
                p.skip = 1;
            }
        };

        if let Some(d) = &mut self.drawing {
            // Inside a picture only a few elements matter.
            match name {
                "blip" => {
                    if let Some(id) = attr(e, "embed") {
                        d.rel = Some(id);
                    } else if let Some(id) = attr(e, "link") {
                        d.rel = Some(id);
                        d.linked = true;
                    }
                }
                "imagedata" => {
                    if let Some(id) = attr(e, "id").or_else(|| attr(e, "relid")) {
                        d.rel = Some(id);
                    }
                }
                "extent" if d.layout.cx == 0 => {
                    d.layout.cx = int_attr(e, "cx").unwrap_or(0).max(0);
                    d.layout.cy = int_attr(e, "cy").unwrap_or(0).max(0);
                }
                "wrapSquare" | "wrapTight" | "wrapThrough" => d.layout.wrap = Wrap::Around,
                "wrapTopAndBottom" => d.layout.wrap = Wrap::TopBottom,
                "wrapNone" => d.layout.wrap = Wrap::Over,
                "positionH" | "positionV" if !empty => {
                    let horizontal = name == "positionH";
                    d.horizontal = Some(horizontal);
                    if horizontal {
                        d.layout.h_from = attr(e, "relativeFrom");
                    }
                }
                "align" if d.horizontal == Some(true) && !empty => d.want = Want::Align,
                "posOffset" if d.horizontal == Some(true) && !empty => d.want = Want::Offset,
                "docPr" => {
                    d.alt = attr(e, "descr")
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| attr(e, "title"))
                        .unwrap_or_default();
                }
                "txbxContent" => {
                    d.text_box = true;
                    self.warn_text_box();
                    skip(self);
                }
                "Fallback" => skip(self),
                _ => {}
            }
            return;
        }

        if self.in_ppr {
            match name {
                "pStyle" => self.para_style_id = val(e),
                // Paragraph-mark formatting and revisions are not content.
                "rPr" | "pPrChange" => skip(self),
                "sectPr" => {
                    self.begin_sect(empty);
                    if let Some(p) = &mut self.para {
                        p.break_after = true;
                    }
                }
                "type" if self.in_sectpr => {
                    // Only a continuous section break keeps the page.
                    if val(e).as_deref() == Some("continuous")
                        && let Some(p) = &mut self.para
                    {
                        p.break_after = false;
                    }
                }
                "pgSz" | "pgMar" if self.in_sectpr => self.sect_prop(name, e),
                _ if self.in_sectpr => {}
                _ => {
                    self.para_direct.apply(name, e);
                }
            }
            return;
        }
        if self.in_trpr {
            let count = || int_attr(e, "val").map_or(0, |n| usize::try_from(n).unwrap_or(0));
            match (name, &mut self.row) {
                ("tblHeader", Some(r)) => r.header = on_off(e),
                ("gridBefore", Some(r)) => r.before = count(),
                ("gridAfter", Some(r)) => r.after = count(),
                // The row as it was before a tracked change.
                ("trPrChange", _) => skip(self),
                _ => {}
            }
            return;
        }
        if self.in_tcpr {
            let merge = || match val(e).as_deref() {
                Some("restart") => Merge::Restart,
                _ => Merge::Continue,
            };
            match (name, self.row.as_mut().and_then(|r| r.props.last_mut())) {
                ("gridSpan", Some(c)) => {
                    c.span = int_attr(e, "val").map_or(1, |n| usize::try_from(n).unwrap_or(1));
                }
                ("vMerge", Some(c)) => c.v_merge = merge(),
                ("hMerge", Some(c)) => c.h_merge = merge(),
                ("textDirection", Some(c)) => {
                    c.rotated = val(e).is_some_and(|d| d != "lrTb" && d != "lrTbV");
                }
                // The cell as it was before a tracked change.
                ("tcPrChange", _) => skip(self),
                _ => {}
            }
            return;
        }
        if self.in_rpr {
            match name {
                "rStyle" => {
                    if let Some(r) = &mut self.run {
                        r.style = val(e);
                    }
                }
                "rPrChange" => skip(self),
                _ => {
                    if let Some(r) = &mut self.run {
                        r.direct.apply(name, e);
                    }
                }
            }
            return;
        }

        match name {
            "del" | "moveFrom" => {
                self.tracked_changes = true;
                skip(self);
            }
            "ins" | "moveTo" => self.tracked_changes = true,
            // The last section's properties are a child of `w:body`.
            "sectPr" => self.begin_sect(empty),
            "pgSz" | "pgMar" if self.in_sectpr => self.sect_prop(name, e),
            "tcPr" if self.table_depth == 1 && !empty && self.row.is_some() => self.in_tcpr = true,
            "delText"
            | "delInstrText"
            | "rPrChange"
            | "pPrChange"
            | "sectPrChange"
            | "tblPrChange"
            | "trPrChange"
            | "tcPrChange"
            | "Fallback"
            | "footnoteRef"
            | "endnoteRef"
            | "separator"
            | "continuationSeparator"
            | "continuationNotice"
            | "commentReference"
            | "annotationRef"
            | "tblPr"
            | "tblGrid"
            | "tcPr"
            | "sdtPr"
            | "sdtEndPr"
            | "fldData" => skip(self),
            "object" => {
                self.env.warnings.push(ImportWarning {
                    kind: WarningKind::ImageUnsupported,
                    location: self.count,
                    section: None,
                    message: format!(
                        "Paragraph {}: an embedded object (e.g. a spreadsheet or formula) was skipped.",
                        self.count
                    ),
    detail: None,
                });
                skip(self);
            }
            "txbxContent" => {
                self.warn_text_box();
                skip(self);
            }
            "footnote" | "endnote" => {
                let kind = if name == "footnote" {
                    NoteKind::Foot
                } else {
                    NoteKind::End
                };
                let special = attr(e, "type").is_some_and(|t| t != "normal");
                match int_attr(e, "id") {
                    Some(id) if !special => self.note = Some((kind, id)),
                    _ => skip(self),
                }
            }
            "p" => {
                self.start_para();
                if empty {
                    self.finish_para();
                }
            }
            "pPr" if self.para.is_some() && self.run.is_none() => self.in_ppr = !empty,
            "tbl" => {
                if !empty {
                    self.table_depth += 1;
                    if self.table_depth == 1 {
                        self.table_start = self.out.len();
                        self.table_nested = false;
                        self.rows.clear();
                    } else {
                        self.table_nested = true;
                    }
                }
            }
            "tr" if self.table_depth == 1 && !empty => {
                self.row = Some(Row {
                    start: self.out.len(),
                    ..Row::default()
                });
            }
            // A row of a table inside a cell: its cells become one line of
            // the outer cell, joined by " · " (`flatten_row`).
            "tr" if self.table_depth == 2 && !empty && self.note.is_none() => {
                self.nested_row = Some(Row {
                    start: self.out.len(),
                    ..Row::default()
                });
            }
            "tc" if self.table_depth == 2 => {
                let at = self.out.len();
                if let Some(r) = &mut self.nested_row {
                    r.cells.push(at);
                }
            }
            "trPr" => {
                if self.table_depth == 1 && !empty {
                    self.in_trpr = true;
                } else {
                    skip(self);
                }
            }
            "tc" if self.table_depth == 1 => {
                let at = self.out.len();
                if let Some(r) = &mut self.row {
                    r.cells.push(at);
                    r.props.push(CellProps::default());
                }
            }
            "r" => {
                self.ensure_resolved();
                self.run = (!empty).then(RunState::default);
            }
            "rPr" if self.run.is_some() => self.in_rpr = !empty,
            "t" => self.in_t = !empty,
            "instrText" => self.in_instr = !empty,
            "tab" | "ptab" if self.run.is_some() => self.push_text("\t"),
            "br" => match attr(e, "type").as_deref() {
                Some("page") => self.page_break(),
                Some("column") => {}
                _ => self.push_piece(Piece::LineBreak),
            },
            "cr" => self.push_piece(Piece::LineBreak),
            "noBreakHyphen" => self.push_text("\u{2011}"),
            // Word's optional hyphen (Ctrl+-, manual hyphenation): the
            // author's own hyphenation point.
            "softHyphen" => self.push_text("\u{AD}"),
            "sym" => {
                // Symbol-font characters (Wingdings ornaments) have no
                // Unicode equivalent we can rely on; any of them alone on a
                // line is an ornament, i.e. a scene break.
                self.push_text("\u{2766}");
            }
            "footnoteReference" | "endnoteReference" => {
                let kind = if name == "footnoteReference" {
                    NoteKind::Foot
                } else {
                    NoteKind::End
                };
                if attr(e, "customMarkFollows").is_some_and(|v| v == "1" || v == "true")
                    && let Some(r) = &mut self.run
                {
                    r.skip_text = true;
                }
                if let Some(blocks) =
                    int_attr(e, "id").and_then(|id| self.env.notes.get(&(kind, id)))
                {
                    self.push_piece(Piece::Note(blocks.clone()));
                }
            }
            "drawing" | "pict" if !empty => {
                self.ensure_resolved();
                self.drawing = Some(Drawing::default());
            }
            "hyperlink" => {
                if !empty {
                    let url = attr(e, "id")
                        .and_then(|id| self.env.rels.get(&id))
                        .filter(|r| r.external)
                        .map(|r| r.target.clone());
                    self.hyperlinks.push(url);
                }
            }
            "fldSimple" => {
                if !empty {
                    let mut f = Field {
                        instr: attr(e, "instr").unwrap_or_default(),
                        in_result: true,
                        ..Field::default()
                    };
                    classify_field(&mut f);
                    self.fields.push(f);
                    self.mark_toc();
                }
            }
            "fldChar" => match attr(e, "fldCharType").as_deref() {
                Some("begin") => self.fields.push(Field::default()),
                Some("separate") => {
                    if let Some(f) = self.fields.last_mut() {
                        f.in_result = true;
                        classify_field(f);
                    }
                    self.mark_toc();
                }
                Some("end") => {
                    self.fields.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        if let Some(d) = &mut self.drawing {
            match name {
                "align" | "posOffset" => d.want = Want::Nothing,
                "positionH" | "positionV" => d.horizontal = None,
                "drawing" | "pict" => self.finish_drawing(),
                _ => {}
            }
            return;
        }
        if self.in_trpr {
            self.in_trpr = name != "trPr";
            return;
        }
        if self.in_tcpr {
            self.in_tcpr = name != "tcPr";
            return;
        }
        match name {
            "pPr" if self.in_ppr => {
                self.in_ppr = false;
                self.resolved = false;
                self.ensure_resolved();
            }
            "sectPr" => self.end_sect(),
            "rPr" if self.in_rpr => self.in_rpr = false,
            "r" => self.run = None,
            "t" => self.in_t = false,
            "instrText" => self.in_instr = false,
            "p" => self.finish_para(),
            "tbl" => {
                if self.table_depth == 1 {
                    self.finish_table();
                }
                self.table_depth = self.table_depth.saturating_sub(1);
            }
            "tr" if self.table_depth == 1 => self.finish_row(),
            "tr" if self.table_depth == 2 => {
                if self.para.is_some() {
                    self.finish_para();
                }
                if let Some(row) = self.nested_row.take() {
                    self.flatten_row(&row);
                }
            }
            "hyperlink" => {
                self.hyperlinks.pop();
            }
            "fldSimple" => {
                self.fields.pop();
            }
            "footnote" | "endnote" => self.note = None,
            _ => {}
        }
    }

    fn text(&mut self, s: &str) {
        if self.skip > 0 {
            return;
        }
        if let Some(d) = &mut self.drawing {
            match d.want {
                Want::Align => d.layout.h_align = Some(s.trim().to_string()),
                Want::Offset => d.layout.h_offset = s.trim().parse().ok(),
                Want::Nothing => {}
            }
            return;
        }
        if self.in_instr {
            if let Some(f) = self.fields.last_mut() {
                f.instr.push_str(s);
            }
        } else if self.in_t {
            self.push_text(s);
        }
    }

    fn start_para(&mut self) {
        if self.para.is_some() {
            // A paragraph inside a paragraph is malformed; keep both.
            self.finish_para();
        }
        self.count += 1;
        // Field instructions never span paragraphs; an unterminated one
        // must not swallow the rest of the document.
        self.fields.retain(|f| f.in_result);
        self.para = Some(Para {
            index: self.count,
            note: self.note,
            in_table: self.table_depth > 0,
            section: self.pages.len(),
            toc: self.fields.iter().any(|f| f.toc),
            ..Para::default()
        });
        self.para_style_id = None;
        self.para_direct = ParaProps::default();
        self.resolved = false;
    }

    /// Apply the paragraph style and direct paragraph formatting.
    fn ensure_resolved(&mut self) {
        if self.resolved {
            return;
        }
        let Some(p) = &mut self.para else { return };
        self.resolved = true;
        let st = self.env.styles.para(self.para_style_id.as_deref());
        let ppr = st.ppr.overlay(&self.para_direct);
        p.role = match (st.role, ppr.outline) {
            (Role::Body, Some(l)) if l < 9 => Role::Heading(l),
            (r, _) => r,
        };
        p.purpose = st.purpose;
        p.style.clone_from(&self.para_style_id);
        p.align = ppr.align.unwrap_or_default();
        // A damaged file may give any number; the longest page is far
        // below this, and the arithmetic on twips stays in range.
        let twips = |v: Option<i64>| v.unwrap_or(0).clamp(-MAX_TWIPS, MAX_TWIPS);
        p.ind_left = twips(ppr.ind_left);
        p.ind_right = twips(ppr.ind_right);
        p.ind_style_left = twips(st.ppr.ind_left);
        p.ind_first = twips(ppr.ind_first);
        p.ind_style_first = twips(st.ppr.ind_first);
        p.space_before = twips(ppr.space_before);
        p.space_after = twips(ppr.space_after);
        p.page_break_before |= ppr.page_break_before == Some(true);
        p.list = match (p.role, ppr.num_id) {
            (Role::Body | Role::Quote, Some(id)) => {
                self.env.numbering.list_kind(id, ppr.num_level.unwrap_or(0))
            }
            _ => None,
        };
        self.para_style = st;
    }

    fn finish_para(&mut self) {
        self.ensure_resolved();
        self.in_ppr = false;
        self.in_sectpr = false;
        self.run = None;
        if let Some(mut p) = self.para.take() {
            p.toc |= self.fields.iter().any(|f| f.toc);
            self.out.push(p);
        }
    }

    /// A table row ended. In the body its paragraphs stay in `out` until the
    /// table ends (`finish_table` reads the rows); in a footnote or endnote,
    /// which holds blocks and no table, the row becomes one paragraph.
    fn finish_row(&mut self) {
        if self.para.is_some() {
            self.finish_para();
        }
        let Some(row) = self.row.take() else { return };
        if self.note.is_some() {
            self.flatten_row(&row);
        } else {
            self.rows.push(row);
        }
    }

    /// Join a row of a table in a note into one paragraph: cells separated
    /// by " · " (an empty cell is "–", so the columns stay apart),
    /// paragraphs in a cell by spaces, a header row bold. A row with no text
    /// is dropped. A row of one cell keeps its paragraphs.
    fn flatten_row(&mut self, row: &Row) {
        if row.cells.len() < 2 || row.start >= self.out.len() {
            return;
        }
        let paras = self.out.split_off(row.start);
        let text = |s: &str| Seg {
            fmt: Fmt::default(),
            link: None,
            caps: false,
            size: None,
            piece: Piece::Text(s.into()),
        };
        let mut segs = Vec::new();
        // Cell starts relative to the row.
        let starts: Vec<usize> = row
            .cells
            .iter()
            .map(|&at| at.saturating_sub(row.start))
            .collect();
        for (c, &at) in starts.iter().enumerate() {
            let end = starts.get(c + 1).copied().unwrap_or(paras.len());
            let cell = paras.get(at..end).unwrap_or_default();
            if c > 0 {
                segs.push(text(" · "));
            }
            let mut first = true;
            for p in cell.iter().filter(|p| p.is_visible()) {
                if !first {
                    segs.push(text(" "));
                }
                segs.extend(p.segs.iter().cloned());
                first = false;
            }
            if first {
                segs.push(text("–"));
            }
        }
        if !paras.iter().any(Para::is_visible) {
            return;
        }
        if row.header {
            segs.iter_mut().for_each(|s| s.fmt.strong = true);
        }
        self.out.push(Para {
            index: paras[0].index,
            note: paras[0].note,
            in_table: true,
            section: paras[0].section,
            toc: paras.iter().any(|p| p.toc),
            page_break_before: paras[0].page_break_before,
            break_after: paras.iter().any(|p| p.break_after),
            segs,
            ..Para::default()
        });
    }

    /// `TableSimplified` for the table whose first paragraph is `location`.
    fn warn_simplified(&mut self, location: usize, what: &str) {
        self.env.warnings.push(ImportWarning {
            kind: WarningKind::TableSimplified,
            location,
            section: None,
            message: format!("Paragraph {location}: a table could not be kept in full: {what}."),
            detail: None,
        });
    }

    /// The outermost table of the body ended. Its paragraphs (`out` from
    /// `table_start`) become one paragraph that carries the table: rows of
    /// cells, each cell the segments of its visible paragraphs with a line
    /// break between them, nothing of the paragraphs' own formatting but the
    /// alignment, placed on Word's grid (`table::resolve`: spans, merges,
    /// header rows). A table that is only layout stays paragraphs, in
    /// reading order (`TableFlattened`): one with a picture in a cell, one
    /// with one column or one cell (a box around text). One with no text is
    /// dropped.
    fn finish_table(&mut self) {
        if self.para.is_some() {
            self.finish_para();
        }
        let rows = std::mem::take(&mut self.rows);
        let start = self.table_start.min(self.out.len());
        let end = self.out.len();
        let mut raw: Vec<RawRow> = Vec::new();
        let (mut picture, mut text, mut several) = (false, false, false);
        let (mut list, mut rotated) = (false, false);
        for (i, row) in rows.iter().enumerate() {
            // The row's paragraphs; stray ones before the first row belong
            // to it.
            let from = if i == 0 {
                start
            } else {
                row.start.clamp(start, end)
            };
            let to = rows.get(i + 1).map_or(end, |n| n.start.clamp(from, end));
            let mut bounds: Vec<usize> = row.cells.iter().map(|&a| a.clamp(from, to)).collect();
            match bounds.first_mut() {
                Some(first) => *first = from,
                None if self.out[from..to].iter().any(Para::is_visible) => bounds.push(from),
                None => continue,
            }
            let mut cells = Vec::new();
            for (c, &a) in bounds.iter().enumerate() {
                let b = bounds.get(c + 1).copied().unwrap_or(to);
                let props = row.props.get(c).copied().unwrap_or_default();
                let mut cell = RawCell {
                    span: props.span,
                    v_merge: props.v_merge,
                    h_merge: props.h_merge,
                    ..RawCell::default()
                };
                let mut first = true;
                // The ordinal of a list item among the ones just before it.
                let (mut before, mut n) = (None, 0);
                for p in self.out[a..b.max(a)].iter().filter(|p| p.shows_content()) {
                    several |= !first;
                    if first {
                        cell.align = p.align;
                    } else {
                        cell.segs.push(plain_seg(Piece::LineBreak));
                    }
                    // A list item keeps its marker as typed text: a cell
                    // holds inlines.
                    if let Some(ordered) = p.list {
                        n = if before == Some(ordered) { n + 1 } else { 1 };
                        let marker = if ordered {
                            format!("{n}. ")
                        } else {
                            "\u{2022} ".to_string()
                        };
                        cell.segs.push(plain_seg(Piece::Text(marker)));
                        list = true;
                    }
                    before = p.list;
                    cell.segs.extend(p.segs.iter().cloned());
                    first = false;
                }
                picture |= cell
                    .segs
                    .iter()
                    .any(|s| matches!(s.piece, Piece::Image { .. }));
                text |= !first;
                rotated |= props.rotated && !first;
                cells.push(cell);
            }
            raw.push(RawRow {
                before: row.before,
                after: row.after,
                cells,
                header: row.header,
            });
        }
        if raw.is_empty() {
            return;
        }
        let grid = table::resolve(raw);
        let location = self.out.get(start).map_or(self.count, |p| p.index);
        if picture || (text && grid.cells_wide < 2) {
            let why = if picture {
                "a picture in a cell"
            } else {
                "one column"
            };
            self.env.warnings.push(ImportWarning {
                kind: WarningKind::TableFlattened,
                location,
                section: None,
                message: format!(
                    "Paragraph {location}: a table with {why} was kept as paragraphs, in reading \
                     order; a table block needs at least two columns."
                ),
                detail: None,
            });
            return;
        }
        if !text {
            self.out.truncate(start);
            self.warn_simplified(location, "it had no text, so it was left out");
            return;
        }
        // What a table block cannot hold is kept as text, and said once.
        let mut lost = Vec::new();
        if several {
            lost.push("a cell with several paragraphs became lines of one cell");
        }
        if list {
            lost.push("a list in a cell became lines with typed markers");
        }
        if self.table_nested {
            lost.push("a table inside a cell became lines of that cell");
        }
        if rotated {
            lost.push("rotated text in a cell became upright");
        }
        if grid.orphan {
            lost.push("a merged cell with nothing to merge into stayed an ordinary cell");
        }
        if grid.covered_text {
            lost.push("text in a covered cell of a merge went to the merged cell");
        }
        if !lost.is_empty() {
            self.warn_simplified(location, &lost.join("; "));
        }
        let data = TableData {
            columns: grid.columns,
            header_rows: grid.header_rows,
            rows: grid
                .rows
                .into_iter()
                .map(|cells| DataRow {
                    cells: cells
                        .into_iter()
                        .map(|c| DataCell {
                            segs: c.segs,
                            align: c.align,
                            col: c.col,
                            colspan: c.colspan,
                            rowspan: c.rowspan,
                        })
                        .collect(),
                })
                .collect(),
        };
        let paras = self.out.split_off(start);
        self.out.push(Para {
            index: paras[0].index,
            note: paras[0].note,
            in_table: true,
            section: paras[0].section,
            // A table is a generated contents list only when all of it is;
            // one cell in a TOC field must not take the table with it.
            toc: paras.iter().filter(|p| p.is_visible()).all(|p| p.toc),
            page_break_before: paras[0].page_break_before,
            break_after: paras.iter().any(|p| p.break_after),
            table: Some(data),
            ..Para::default()
        });
    }

    /// `<w:br w:type="page"/>`: mark the paragraph if nothing visible came
    /// before the break, otherwise split it.
    fn page_break(&mut self) {
        let Some(p) = &mut self.para else { return };
        if !p.is_visible() {
            p.page_break_before = true;
            return;
        }
        let mut rest = Para {
            segs: Vec::new(),
            page_break_before: true,
            ..p.clone()
        };
        std::mem::swap(p, &mut rest);
        // `rest` is now the part before the break.
        rest.break_after = false;
        self.out.push(rest);
    }

    fn mark_toc(&mut self) {
        if self.fields.iter().any(|f| f.toc)
            && let Some(p) = &mut self.para
        {
            p.toc = true;
        }
    }

    fn warn_text_box(&mut self) {
        self.env.warnings.push(ImportWarning {
            kind: WarningKind::TextBoxSkipped,
            location: self.count,
            section: None,
            message: format!(
                "Paragraph {}: the text in a text box was skipped. Move it into the text if it belongs to the book.",
                self.count
            ),
    detail: None,
        });
    }

    /// `w:sectPr` starts: a new set of section properties.
    fn begin_sect(&mut self, empty: bool) {
        self.sect = Sect::default();
        self.in_sectpr = !empty;
        if empty {
            self.pages.push(self.sect.page());
        }
    }

    /// `w:sectPr` ends: the section's text block is known.
    fn end_sect(&mut self) {
        if self.in_sectpr {
            self.pages.push(self.sect.page());
        }
        self.in_sectpr = false;
    }

    fn sect_prop(&mut self, name: &str, e: &BytesStart) {
        let twips = |local| int_attr(e, local);
        match name {
            "pgSz" => {
                self.sect.w = twips("w");
                self.sect.h = twips("h");
            }
            _ => {
                self.sect.left = twips("left");
                self.sect.right = twips("right");
                self.sect.top = twips("top");
                self.sect.bottom = twips("bottom");
                self.sect.gutter = twips("gutter");
            }
        }
    }

    fn finish_drawing(&mut self) {
        let Some(d) = self.drawing.take() else { return };
        let Some(rel_id) = d.rel else {
            if d.text_box {
                return; // already reported
            }
            // Shapes, charts and SmartArt without a picture.
            self.env.warnings.push(ImportWarning {
                kind: WarningKind::ImageUnsupported,
                location: self.count,
                section: None,
                message: format!(
                    "Paragraph {}: a drawing that is not a picture (shape, chart or SmartArt) was skipped.",
                    self.count
                ),
    detail: None,
            });
            return;
        };
        let rel = self.env.rels.get(&rel_id).cloned();
        let rel = rel.as_ref();
        let env = &mut *self.env;
        let asset = env
            .media
            .image(env.pkg, rel, d.linked, self.count, env.warnings);
        if let Some(asset) = asset {
            self.push_piece(Piece::Image {
                asset,
                alt: d.alt,
                layout: d.layout,
                place: Place::default(),
            });
        }
    }

    fn text_allowed(&self) -> bool {
        self.para.is_some() && self.fields.iter().all(|f| f.in_result)
    }

    fn push_text(&mut self, s: &str) {
        if !self.text_allowed() || self.run.as_ref().is_some_and(|r| r.skip_text) {
            return;
        }
        let Some((fmt, caps, size)) = self.run_format() else {
            return; // hidden text
        };
        let link = self.link();
        let Some(p) = &mut self.para else { return };
        if let Some(last) = p.segs.last_mut()
            && last.fmt == fmt
            && last.caps == caps
            && last.size == size
            && last.link == link
            && let Piece::Text(t) = &mut last.piece
        {
            t.push_str(s);
            return;
        }
        p.segs.push(Seg {
            fmt,
            link,
            caps,
            size,
            piece: Piece::Text(s.to_string()),
        });
    }

    fn push_piece(&mut self, piece: Piece) {
        if !self.text_allowed() {
            return;
        }
        self.ensure_resolved();
        let Some(p) = &mut self.para else { return };
        // Marks and breaks take the formatting of the text before them, so
        // a footnote inside an italic phrase does not split the emphasis.
        let (fmt, link) = p
            .segs
            .last()
            .map(|s| (s.fmt, s.link.clone()))
            .unwrap_or_default();
        p.segs.push(Seg {
            fmt: Fmt {
                sup: false,
                sub: false,
                ..fmt
            },
            link,
            caps: false,
            size: None,
            piece,
        });
    }

    fn link(&self) -> Option<String> {
        self.fields
            .iter()
            .rev()
            .find_map(|f| f.link.clone())
            .or_else(|| self.hyperlinks.iter().rev().find_map(Clone::clone))
    }

    /// Effective formatting, capitals and font size of the current run,
    /// `None` if hidden.
    fn run_format(&mut self) -> Option<(Fmt, bool, Option<u16>)> {
        let (direct, style) = match &self.run {
            Some(r) => (r.direct, r.style.clone()),
            None => (RunProps::default(), None),
        };
        let cs = match style {
            Some(id) => *self
                .char_styles
                .entry(id.clone())
                .or_insert_with(|| self.env.styles.char_rpr(&id)),
            None => RunProps::default(),
        };
        let ps = self.para_style.rpr;
        // Toggle properties: paragraph and character style values XOR,
        // direct formatting is absolute (ECMA-376 §17.7.3).
        let toggle = |d: Option<bool>, p: Option<bool>, c: Option<bool>| {
            d.unwrap_or_else(|| p.unwrap_or(false) ^ c.unwrap_or(false))
        };
        if toggle(direct.hidden, ps.hidden, cs.hidden) {
            return None;
        }
        // In headings, quotes, verse etc. the style's own italic/bold is
        // the theme's business; only deviations from it are emphasis.
        let semantic = self.para.as_ref().is_some_and(|p| p.role != Role::Body);
        let base = |v: Option<bool>| semantic && v.unwrap_or(false);
        let italic = toggle(direct.italic, ps.italic, cs.italic) != base(ps.italic);
        let bold = toggle(direct.bold, ps.bold, cs.bold) != base(ps.bold);
        let small_caps =
            toggle(direct.small_caps, ps.small_caps, cs.small_caps) != base(ps.small_caps);
        let caps = toggle(direct.caps, ps.caps, cs.caps);
        // Manuscript convention (Shunn format): underline means italics.
        // Only direct underline counts; link styles underline by style.
        let underline = direct.underline == Some(true) && self.link().is_none();
        let vert = direct.vert.or(cs.vert).or(ps.vert);
        Some((
            Fmt {
                em: italic || underline,
                strong: bold,
                small_caps,
                sup: vert == Some(VertAlign::Super),
                sub: vert == Some(VertAlign::Sub),
            },
            caps,
            direct.size.or(cs.size).or(ps.size),
        ))
    }
}

/// Recognize TOC and HYPERLINK fields from their instruction text.
fn classify_field(f: &mut Field) {
    let instr = f.instr.trim_start();
    let word = instr
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match word.as_str() {
        "TOC" => f.toc = true,
        "HYPERLINK" => {
            // HYPERLINK "url" [switches]; `\l "anchor"` is an internal link.
            let rest = instr[word.len()..].trim_start();
            if !rest.starts_with('\\') {
                f.link = rest.split('"').nth(1).map(str::to_string);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docx::media::Media;
    use crate::docx::package::Package;

    pub(crate) const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

    fn parse_body(body: &str, styles: &Styles) -> (Vec<Para>, Vec<ImportWarning>) {
        let xml = format!(r#"<w:document {NS}><w:body>{body}</w:body></w:document>"#);
        let zip = crate::docx::tests::empty_zip();
        let mut pkg = Package::open(&zip).unwrap();
        let mut media = Media::default();
        let mut warnings = Vec::new();
        let rels = HashMap::new();
        let notes = HashMap::new();
        let numbering = Numbering::default();
        let mut env = Env {
            pkg: &mut pkg,
            styles,
            numbering: &numbering,
            rels: &rels,
            notes: &notes,
            media: &mut media,
            warnings: &mut warnings,
        };
        let paras = parse(&xml, &mut env);
        (paras, warnings)
    }

    fn texts(p: &Para) -> Vec<(String, Fmt)> {
        p.segs
            .iter()
            .filter_map(|s| s.text().map(|t| (t.to_string(), s.fmt)))
            .collect()
    }

    const EM: Fmt = Fmt {
        em: true,
        strong: false,
        small_caps: false,
        sup: false,
        sub: false,
    };

    #[test]
    fn runs_merge_and_toggle() {
        let styles = Styles::parse(&format!(
            r#"<w:styles {NS}>
  <w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:rPr><w:i/></w:rPr></w:style>
  <w:style w:type="character" w:styleId="Emph"><w:name w:val="Emphasis"/><w:rPr><w:i/></w:rPr></w:style>
</w:styles>"#
        ));
        let (paras, _) = parse_body(
            r#"<w:p><w:r><w:t xml:space="preserve">Es war </w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>ein</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>mal</w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:t xml:space="preserve"> &amp; so</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t xml:space="preserve">Zitat mit </w:t></w:r><w:r><w:rPr><w:rStyle w:val="Emph"/></w:rPr><w:t>Betonung</w:t></w:r></w:p>"#,
            &styles,
        );
        assert_eq!(paras.len(), 2);
        assert_eq!(
            texts(&paras[0]),
            vec![
                ("Es war ".into(), Fmt::default()),
                ("einmal".into(), EM),
                (" & so".into(), Fmt::default())
            ]
        );
        // Italic quote style + italic char style = roman = emphasis.
        assert_eq!(paras[1].role, Role::Quote);
        assert_eq!(
            texts(&paras[1]),
            vec![
                ("Zitat mit ".into(), Fmt::default()),
                ("Betonung".into(), EM)
            ]
        );
    }

    #[test]
    fn tracked_changes_fields_and_hidden_text() {
        let (paras, warnings) = parse_body(
            r#"<w:p><w:r><w:t>A</w:t></w:r><w:del w:id="1"><w:r><w:delText>gone</w:delText></w:r></w:del><w:ins w:id="2"><w:r><w:t>B</w:t></w:r></w:ins>
<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>C</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>
<w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden</w:t></w:r></w:p>"#,
            &Styles::default(),
        );
        assert_eq!(texts(&paras[0]), vec![("ABC".into(), Fmt::default())]);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::TrackedChangesAccepted)
        );
    }

    #[test]
    fn page_breaks_split_paragraphs() {
        let (paras, _) = parse_body(
            r#"<w:p><w:r><w:t>End.</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:t>Next</w:t></w:r></w:p>
<w:p><w:r><w:br w:type="page"/><w:t>Top</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr><w:r><w:t>Last</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr><w:type w:val="continuous"/></w:sectPr></w:pPr></w:p>"#,
            &Styles::default(),
        );
        let summary: Vec<_> = paras
            .iter()
            .map(|p| (p.index, p.page_break_before, p.break_after, texts(p).len()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (1, false, false, 1),
                (1, true, false, 1),
                (2, true, false, 1),
                (3, false, true, 1),
                (4, false, false, 0),
            ]
        );
    }

    #[test]
    fn toc_fields_are_marked_and_text_boxes_skipped() {
        let (paras, warnings) = parse_body(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>TOC \o "1-3"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Kapitel 1 ..... 3</w:t></w:r></w:p>
<w:p><w:r><w:t>Kapitel 2 ..... 9</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
<w:p><w:r><w:t>Body</w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback><w:pict><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed</w:t></w:r></w:p></w:txbxContent></v:textbox></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
            &Styles::default(),
        );
        assert_eq!(paras.len(), 3);
        assert!(paras[0].toc && paras[1].toc && !paras[2].toc);
        assert_eq!(texts(&paras[2]), vec![("Body".into(), Fmt::default())]);
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.kind == WarningKind::TextBoxSkipped)
                .count(),
            1
        );
    }

    #[test]
    fn hyperlink_fields_and_underline() {
        let (paras, _) = parse_body(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "https://example.com" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>site</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>under</w:t></w:r></w:p>"#,
            &Styles::default(),
        );
        let segs = &paras[0].segs;
        assert_eq!(segs[0].link.as_deref(), Some("https://example.com"));
        assert!(!segs[0].fmt.em, "underlined link is not emphasis");
        assert!(segs[1].fmt.em, "underline means italics in manuscripts");
    }

    #[test]
    fn damaged_xml_keeps_what_was_read() {
        let styles = Styles::default();
        let (paras, warnings) = parse_body(
            r#"<w:p><w:r><w:t>Fine</w:t></w:r></w:p><w:p><w:r><w:t>Cut off</w:t></w:r></w:p></w:x>"#,
            &styles,
        );
        assert_eq!(paras.len(), 2);
        assert!(warnings.iter().any(|w| w.kind == WarningKind::DamagedFile));
    }
}
