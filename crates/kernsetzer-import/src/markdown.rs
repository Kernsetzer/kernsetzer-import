//! The Kernsetzer Markdown dialect (M1 extends the M0 subset, see checklist).
//!
//! ```text
//! Title: Der Leuchtturm          <- front matter: `Key: value` lines at the top
//! Author: Lena Brandt               keys: Title, Subtitle, Author, Lang,
//! Lang: de                          Publisher, Copyright (or Rights),
//!                                   Dedication; or a YAML block between
//!                                   `---` lines (see `front_matter.rs`).
//!                                   Without `Lang:`, English or German is
//!                                   detected from the text, as for .docx
//!
//! *For Ada, who asked first.*    <- before the first heading: a paragraph
//!                                   starting like a dedication (For, To, Für,
//!                                   In memory, Gewidmet, …) is a Dedication
//!                                   section; up to three more right after it
//!                                   join it, 300 characters in all. Italic
//!                                   over a whole paragraph is dropped
//!
//! # Chapter title                <- new chapter
//! # Prolog / # Prologue          <- recognized section kinds (also Epilog, …)
//! # Interlude {-}                <- an unnumbered chapter (also `{.unnumbered}`,
//!                                   as in Pandoc): no label, counting skips it
//! ## Subheading                  <- heading inside the section; `##` is level 1,
//!                                   `###` level 2, … up to `######`
//!
//! > Quoted text, lines joined.   <- block quote; a bare `>` line separates
//! >                                 paragraphs, a blank line ends the quote
//! > Second paragraph.
//!
//! Paragraphs are separated by blank lines. *Emphasis*, _emphasis_, **strong**,
//! [small caps]{.smallcaps} (a Pandoc span; other `{…}` attributes are dropped),
//! 2^nd^, H~2~O (superscript, subscript: no spaces inside, `~~` is text),
//! [links](https://example.com "title") and <https://example.com>.
//!
//! Dear Anna,\                    <- a trailing `\` is a line break (paragraphs,
//! the house is sold.                quotes, notes); on a last line it is dropped
//!
//! :::verse                       <- verse: every line is kept, a trailing `\`
//! How doth the little crocodile     is allowed; blank lines separate stanzas;
//!     Improve his shining tail,     ends at `:::` (or, with a warning, before
//!                                   the next heading or at the end). Leading
//! How cheerfully he seems to grin,  spaces indent a line: one level per four
//! :::                               (a tab is four), counted from the least-
//!                                   indented line, up to six levels
//!
//! :::epigraph                    <- epigraph: text joined like a paragraph
//! Every coast is a rumour\          (blank lines become line breaks); a last
//! until someone draws it twice.     line starting with a dash is the
//! — Harbour saying                  attribution, plain text. After a `#`
//! :::                               heading: the section's epigraph; before
//!                                   the first heading: an Epigraph section;
//!                                   elsewhere a block quote. Ends like verse
//!
//! ![Map of the island](img/map.png "The island, 1921")
//!                                <- image on a line of its own: alt text, path
//!                                   (relative to the manuscript; `<…>` or plain,
//!                                   spaces allowed), optional "caption"; Pandoc
//!                                   attributes after it size and place it:
//!                                   `{width=50%}` (rounded to 5, 10–100),
//!                                   `{.beside}` + `.outside` `.inside` `.left`
//!                                   `.right`, `{.page}`, `{.bleed}`, combined
//!                                   as `{.beside .left width=35%}`; others
//!                                   are ignored
//!
//! Table: Lydia, my love          <- caption (Pandoc): a `Table: text` line
//! | Day | Miles |                   alone in its paragraph, right above
//! |:----|------:|                   the table or after one blank line, or
//! | 1   | 14    |                   the same below it; shown above, and one
//!                                   above wins over one below. This is a
//!                                   pipe table: a header row, a delimiter
//!                                   row, rows; outer pipes optional, `\|`
//!                                   is text, cells hold emphasis, links and
//!                                   notes. The delimiter row sets each
//!                                   column's alignment (`:--` left, `--:`
//!                                   right, `:-:` centre, `---` left). The
//!                                   header row is the header, none when all
//!                                   of it is empty (`| | |`). A short row
//!                                   is padded; cells beyond the header's
//!                                   count are left out, and a table with no
//!                                   text is dropped: `TableSimplified`.
//!                                   Merged cells cannot be written here
//!
//! - Rope, *the long one*,        <- list: `-`, `*` or `+` items, or `1.`/`1)`;
//!   and the hooks.                  lines joined; after a blank line another
//! - Matches                         item continues it and an indented line
//!                                   (2 spaces, tab) is a second paragraph;
//!   Keep them dry.                  deeper-indented items nest. Starts after
//!   - in the tin box                a blank line or a block, never inside a
//! 1. Walk to the pier.              paragraph. Ordered: starts at 1, two items
//! 2. Wait for the tide.             at least ("3. Oktober …" is text).
//!                                   Items counting on from another number
//!                                   ("8.", "9.") are numbered paragraphs,
//!                                   with a `ListFlattened` warning
//!
//! A footnote mark[^1] goes on the word; the definition can be anywhere:
//!
//! [^1]: Note text, lines joined; after a blank line,
//!     an indented line (tab or 4 spaces) starts its second paragraph.
//!
//! Ver&shy;legenheit             <- soft hyphen (or U+00AD itself): the
//!                                   author's own hyphenation point
//!
//! <!-- TODO: the year -->        <- HTML comment: never printed, also over
//!                                   several lines; an unclosed `<!--` is text
//!
//! ***                            <- scene break (also `* * *`, `#`, `~~~`)
//!
//! \newpage                       <- page break (also `\pagebreak`); dropped at
//!                                   the start or end of a section
//! ```

pub(crate) mod front_matter;

use std::borrow::Cow;
use std::collections::HashMap;

use crate::{
    ImportError, ImportWarning, Imported, MAX_IMAGE_BYTES, WarningDetail, WarningKind, assets,
    heading,
};
use front_matter::FrontMatter;
use setzer_model::asset::{Asset, ImageFormat};
use setzer_model::{
    Block, Book, ColumnAlign, Epigraph, ImagePlacement, ImageSide, Inline, Lang, MAX_VERSE_INDENT,
    Numbering, Section, SectionKind, Table, TableCell, TableColumn,
};

/// Import a manuscript. The parser does no I/O: `files` returns the bytes of
/// an image path as written in the manuscript (relative to it), or `None`
/// if there is no such file. Each path is asked for once. Pass `|_| None`
/// when no files are at hand; images then warn as missing.
pub fn import(
    src: &str,
    mut files: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Imported, ImportError> {
    let src = soft_hyphens(strip_comments(src));
    if src.trim().is_empty() {
        return Err(ImportError::Empty);
    }
    let lines: Vec<&str> = src.lines().collect();
    let FrontMatter {
        mut meta,
        lang_given,
        dedication,
        end: mut i,
        warnings,
    } = front_matter::read(&lines);
    // Without a `Lang:` line, the text says (as for .docx).
    if !lang_given && let Some(lang) = crate::lang::detect(lines[i..].iter().copied()) {
        meta.lang = Lang(lang.into());
    }

    let mut lines = lines;
    let mut body = Body {
        sections: Vec::new(),
        warnings,
        notes: Notes::collect(&mut lines[i..], i),
        files: &mut files,
        assets: Vec::new(),
        images: HashMap::new(),
        front: 0,
        dedication: None,
    };
    if let Some(d) = dedication {
        let blocks = vec![Block::paragraph(body.inlines(&d))];
        body.sections.push(Section {
            style: Default::default(),
            kind: SectionKind::Dedication,
            title: None,
            numbering: Numbering::None,
            epigraph: None,
            blocks,
        });
        body.note_warnings(1);
        body.front = body.sections.len();
    }

    // Lines of the paragraph being read, and the line number it starts on.
    let mut para: Vec<&str> = Vec::new();
    let mut para_start = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let line_no = i + 1;
        if let Some(h) = line.strip_prefix("# ") {
            body.flush(&mut para, para_start);
            // "Kapitel 3" / "Chapter 3: Title": the number is stored as
            // explicit numbering and the label is generated by the theme.
            let (h, unnumbered) = unnumbered(h);
            let info = heading::classify(h);
            body.sections.push(Section {
                style: Default::default(),
                kind: info.kind,
                numbering: if unnumbered {
                    Numbering::None
                } else {
                    info.numbering()
                },
                title: info.title,
                epigraph: None,
                blocks: Vec::new(),
            });
        } else if let Some((level, text)) = sub_heading(line) {
            body.flush(&mut para, para_start);
            let block = Block::heading(level, body.inlines(text));
            body.push(block, line_no);
        } else if let Some(image) = image_line(line) {
            body.flush(&mut para, para_start);
            body.image(&image, line_no);
        } else if let Some(kind) = fence(line) {
            body.flush(&mut para, para_start);
            i = match kind {
                Fence::Verse => body.verse(&lines, i),
                Fence::Epigraph => body.epigraph(&lines, i),
            };
            continue;
        } else if line.starts_with('>') {
            body.flush(&mut para, para_start);
            let (quote, next) = block_quote(&lines, i, &mut body.notes);
            if let Some(block) = quote {
                body.push(block, line_no);
            }
            i = next;
            continue;
        } else if para.is_empty()
            && let Some((caption, pipe_table, next)) = caption_above(&lines, i)
        {
            body.table(&pipe_table, Some(caption));
            i = next;
            continue;
        } else if let Some((pipe_table, mut next)) = table(&lines, i) {
            body.flush(&mut para, para_start);
            let caption = caption_below(&lines, next).map(|(caption, after)| {
                next = after;
                caption
            });
            body.table(&pipe_table, caption);
            i = next;
            continue;
        } else if heading::is_scene_break(line) {
            body.flush(&mut para, para_start);
            if let Some(s) = body.current() {
                // Collapse repeated breaks and drop a break at chapter start.
                if !s.blocks.is_empty()
                    && !matches!(s.blocks.last(), Some(Block::SceneBreak { .. }))
                {
                    s.blocks.push(Block::scene_break());
                }
            }
        } else if is_page_break(line) {
            body.flush(&mut para, para_start);
            if let Some(s) = body.current() {
                // Collapse repeated breaks and drop a break at section start
                // (sections start on a new page anyway).
                if !s.blocks.is_empty() && !matches!(s.blocks.last(), Some(Block::PageBreak { .. }))
                {
                    s.blocks.push(Block::page_break());
                }
            }
        } else if para.is_empty()
            && let Some((run, next)) = list(&lines, i, &mut body.notes)
        {
            match run {
                ListRun::Lists(lists) => {
                    for block in lists {
                        body.push(block, line_no);
                    }
                }
                ListRun::Numbered { start, paras } => body.numbered(start, &paras),
            }
            i = next;
            continue;
        } else if line.is_empty() {
            body.flush(&mut para, para_start);
        } else {
            if para.is_empty() {
                para_start = line_no;
            }
            para.push(line);
        }
        i += 1;
    }
    body.flush(&mut para, para_start);
    body.unused_note_warnings();
    let Body {
        mut sections,
        mut warnings,
        assets,
        ..
    } = body;

    // A scene or page break as the last block of a chapter is a manuscript
    // artifact.
    for s in &mut sections {
        while matches!(
            s.blocks.last(),
            Some(Block::SceneBreak { .. } | Block::PageBreak { .. })
        ) {
            s.blocks.pop();
        }
    }

    if meta.title.is_empty() {
        warnings.push(ImportWarning {
            kind: WarningKind::NoTitle,
            location: 1,
            section: None,
            message: "No `Title:` line found; the book has no title.".into(),
            detail: None,
        });
    }

    let mut book = Book::new(meta);
    book.sections = sections;
    Ok(Imported {
        book,
        assets,
        warnings,
    })
}

/// `src` without its HTML comments (`<!-- … -->`, also over several
/// lines), which never render (CommonMark, Pandoc). The line breaks inside
/// a comment are kept, so line numbers stay right; a comment on lines of
/// its own leaves blank lines and so ends a paragraph, as an HTML block
/// does in CommonMark. An unclosed `<!--` stays text: it would hide the
/// rest of the book, and as text pre-flight finds it.
pub(crate) fn strip_comments(src: &str) -> Cow<'_, str> {
    if !src.contains("<!--") {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(open) = rest.find("<!--") {
        let Some(len) = rest[open + 4..].find("-->") else {
            break;
        };
        let comment = &rest[open..open + 4 + len + 3];
        out.push_str(&rest[..open]);
        rest = &rest[open + comment.len()..];
        let breaks = comment.matches('\n').count();
        out.extend(std::iter::repeat_n('\n', breaks));
        // "a <!-- x --> b" keeps one space.
        if breaks == 0 && out.ends_with([' ', '\t']) {
            rest = rest.trim_start_matches([' ', '\t']);
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// `src` with `&shy;` as U+00AD, the soft hyphen: an author's own
/// hyphenation point, which is invisible in most editors, so it is typed as
/// the HTML entity (Pandoc reads it too). The dialect has no other entities.
fn soft_hyphens(src: Cow<'_, str>) -> Cow<'_, str> {
    if src.contains("&shy;") {
        Cow::Owned(src.replace("&shy;", "\u{AD}"))
    } else {
        src
    }
}

/// The book body as it is read, with the warnings, footnotes and images
/// so far.
struct Body<'a> {
    sections: Vec<Section>,
    warnings: Vec<ImportWarning>,
    notes: Notes,
    files: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    assets: Vec<Asset>,
    /// Path as written → asset id (`None` if the image could not be used).
    images: HashMap<String, Option<String>>,
    /// Sections made from front matter (the dedication); body text never
    /// goes into them.
    front: usize,
    /// A `Dedication` section made from paragraphs, which a short
    /// paragraph right after it joins (see [`Self::dedication`]).
    dedication: Option<usize>,
}

impl Body<'_> {
    /// The section body blocks go into: the last one opened by a heading.
    fn current(&mut self) -> Option<&mut Section> {
        if self.sections.len() > self.front {
            self.sections.last_mut()
        } else {
            None
        }
    }

    /// Index of [`Self::current`], for warnings.
    fn current_index(&self) -> Option<usize> {
        self.sections
            .len()
            .checked_sub(1)
            .filter(|&i| i >= self.front)
    }

    fn inlines(&mut self, s: &str) -> Vec<Inline> {
        parse_inlines(s, &mut self.notes)
    }

    /// End the running paragraph, if any; it started on line `start`.
    fn flush(&mut self, para: &mut Vec<&str>, start: usize) {
        if para.is_empty() {
            return;
        }
        let text = join_lines(para);
        para.clear();
        let block = Block::paragraph(self.inlines(&text));
        self.push(block, start);
    }

    /// Append a body block to the current section; text before the first
    /// `#` heading is a dedication or opens an untitled section, with a
    /// warning.
    fn push(&mut self, block: Block, line_no: usize) {
        match self.current() {
            Some(s) => s.blocks.push(block),
            None => {
                if let Some(block) = self.dedication(block) {
                    self.warnings.push(ImportWarning {
                        kind: WarningKind::TextBeforeFirstChapter,
                        location: line_no,
                        section: Some(self.sections.len()),
                        message: "Text before the first chapter heading was put into an \
                                  untitled section."
                            .into(),
                        detail: None,
                    });
                    self.sections.push(Section {
                        style: Default::default(),
                        kind: SectionKind::Other,
                        title: None,
                        numbering: Numbering::None,
                        epigraph: None,
                        blocks: vec![block],
                    });
                }
            }
        }
        self.note_warnings(line_no);
    }

    /// Put a paragraph before the first heading into a dedication, by the
    /// .docx rule: one that starts like a dedication ("For …", "Für …",
    /// "In memory …") opens a `Dedication` section, and short ones right
    /// after it join it, up to [`DEDICATION_PARAS`] paragraphs and
    /// [`DEDICATION_CHARS`] characters. Italic over a whole paragraph is
    /// the dedication's style and dropped. Returns any other block.
    fn dedication(&mut self, block: Block) -> Option<Block> {
        let Block::Paragraph { content, .. } = &block else {
            return Some(block);
        };
        let chars = |c: &[Inline]| Inline::plain_text(c).trim().chars().count();
        let len = chars(content);
        let open = self
            .dedication
            .filter(|&d| d + 1 == self.sections.len())
            .and_then(|d| self.sections.get(d));
        let joins = open.is_some_and(|s| {
            let so_far: usize = s
                .blocks
                .iter()
                .map(|b| match b {
                    Block::Paragraph { content, .. } => chars(content),
                    _ => 0,
                })
                .sum();
            s.blocks.len() < DEDICATION_PARAS && so_far + len <= DEDICATION_CHARS
        });
        let opens =
            len <= DEDICATION_CHARS && crate::is_dedication(Inline::plain_text(content).trim());
        if !joins && !opens {
            return Some(block);
        }
        let para = Block::paragraph(match content.as_slice() {
            [Inline::Emphasis { content: inner }] => inner.clone(),
            _ => content.clone(),
        });
        match self.sections.last_mut() {
            Some(s) if joins => s.blocks.push(para),
            _ => {
                self.sections.push(Section {
                    style: Default::default(),
                    kind: SectionKind::Dedication,
                    title: None,
                    numbering: Numbering::None,
                    epigraph: None,
                    blocks: vec![para],
                });
                self.front = self.sections.len();
                self.dedication = Some(self.front - 1);
            }
        }
        None
    }

    /// Add the items of a list that starts at `start`, not 1, as numbered
    /// paragraphs (line index, lines), and warn once: the model has no
    /// start number.
    fn numbered(&mut self, start: u32, paras: &[(usize, Vec<&str>)]) {
        for (line, lines) in paras {
            let block = Block::paragraph(self.inlines(&join_lines(lines)));
            self.push(block, line + 1);
        }
        let Some(&(first, _)) = paras.first() else {
            return;
        };
        self.warnings.push(ImportWarning {
            kind: WarningKind::ListFlattened,
            location: first + 1,
            section: self.current_index(),
            message: format!(
                "This numbered list starts at {start}, so its items were kept as paragraphs \
                 with their numbers: lists are numbered from 1."
            ),
            detail: None,
        });
    }

    /// Add an image block, or warn and leave it out if its file cannot be
    /// used. A path used again shares the asset (and warns only once).
    fn image(&mut self, image: &ImageLine, line_no: usize) {
        let id = match self.images.get(image.path) {
            Some(id) => id.clone(),
            None => {
                let id = self.asset(image.path, line_no);
                self.images.insert(image.path.to_string(), id.clone());
                id
            }
        };
        let Some(id) = id else {
            return;
        };
        let alt = Inline::plain_text(&parse_inlines(image.alt, &mut Notes::default()));
        let caption = image
            .caption
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| self.inlines(c));
        let (width, placement, side) = image_layout(image.attrs);
        let mut block = Block::image(id, alt.trim(), caption)
            .with_placement(placement)
            .with_side(side);
        if let Some(width) = width {
            block = block.with_width(width);
        }
        self.push(block, line_no);
    }

    /// Add a pipe table as a table block: the delimiter row
    /// gives each column's alignment, the header row is the one header row
    /// (none when all of it is empty: how GFM writes a table without a
    /// header), a short row is padded and cells beyond the header's count are
    /// left out. A table with no text at all is dropped (its caption stays
    /// a paragraph). Both cases warn with `TableSimplified`, once per table.
    /// The caption, if any, is the table's caption (inlines).
    fn table(&mut self, table: &PipeTable, caption: Option<Caption<'_>>) {
        let Some(&(start, _)) = table.rows.first() else {
            return;
        };
        let width = table.aligns.len();
        // The section is read when the warning is given, after the table (or
        // its caption) has opened one if the manuscript had no heading yet.
        let warn = |section: Option<usize>, message: &str| ImportWarning {
            kind: WarningKind::TableSimplified,
            location: start + 1,
            section,
            message: message.into(),
            detail: None,
        };
        if table
            .rows
            .iter()
            .all(|(_, cells)| cells.iter().all(String::is_empty))
        {
            if let Some(caption) = caption {
                let block = Block::paragraph(self.inlines(caption.text));
                self.push(block, caption.line + 1);
            }
            let section = self.current_index();
            self.warnings
                .push(warn(section, "This table has no text, so it was left out."));
            return;
        }
        // A caption above is read before the cells, below after them: the
        // footnotes are numbered in reading order.
        let above = caption.filter(|c| c.above).map(|c| self.inlines(c.text));
        let surplus = table
            .rows
            .iter()
            .any(|(_, cells)| cells.iter().skip(width).any(|c| !c.is_empty()));
        let header = table.rows[0].1.iter().any(|c| !c.is_empty());
        let mut rows = Vec::with_capacity(table.rows.len());
        for (n, (_, cells)) in table.rows.iter().enumerate() {
            if n == 0 && !header {
                continue;
            }
            rows.push(
                cells
                    .iter()
                    .take(width)
                    .map(|cell| TableCell::new(self.inlines(cell)))
                    .collect(),
            );
        }
        let columns = table.aligns.iter().map(|&a| TableColumn::new(a)).collect();
        let mut table_block = Table::new(columns, u32::from(header), rows);
        let below = caption.filter(|c| !c.above).map(|c| self.inlines(c.text));
        if let Some(caption) = above.or(below) {
            table_block = table_block.with_caption(caption);
        }
        self.push(Block::table(table_block), start + 1);
        if surplus {
            let section = self.current_index();
            self.warnings.push(warn(
                section,
                "Some rows of this table have more cells than its header row; the extra \
                 cells were left out.",
            ));
        }
    }

    /// The asset id for an image path, reading the file through `files`.
    fn asset(&mut self, path: &str, line_no: usize) -> Option<String> {
        let section = self.current_index();
        let mut warn = |kind, message: String| {
            self.warnings.push(ImportWarning {
                kind,
                location: line_no,
                section,
                message,
                detail: Some(WarningDetail::Image {
                    name: Some(path.to_string()),
                }),
            });
        };
        if path.contains("://") || path.starts_with("data:") {
            warn(
                WarningKind::ImageMissing,
                format!(
                    "The image `{path}` is a web address. Kernsetzer works offline: save the \
                     image next to the manuscript and use its file name."
                ),
            );
            return None;
        }
        let Some(bytes) = (self.files)(path) else {
            warn(
                WarningKind::ImageMissing,
                format!(
                    "The image file `{path}` was not found (paths are relative to the manuscript)."
                ),
            );
            return None;
        };
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            warn(
                WarningKind::ImageMissing,
                format!("The image file `{path}` is too large (over 100 MB)."),
            );
            return None;
        }
        match ImageFormat::sniff(&bytes) {
            ImageFormat::Supported { ext, .. } => {
                let id = assets::unique_id(path, ext, &self.assets);
                self.assets.push(Asset::new(id.clone(), bytes));
                Some(id)
            }
            ImageFormat::Unsupported(name) => {
                warn(
                    WarningKind::ImageUnsupported,
                    format!(
                        "The image `{path}` is in {name} format, which cannot be used in a \
                         book. Replace it with a PNG or JPEG."
                    ),
                );
                None
            }
        }
    }

    /// The verse fence opening at `lines[start]`; returns the index of the
    /// first line after it. Each line is a verse line, a trailing `\` is
    /// redundant and dropped, blank lines separate stanzas (one empty line in
    /// the model, none at the ends). Leading spaces give the indent level
    /// ([`verse_levels`]).
    fn verse(&mut self, lines: &[&str], start: usize) -> usize {
        let line_no = start + 1;
        let (inner, next, closed) = fence_body(lines, start);
        let texts: Vec<(usize, &str)> = inner
            .iter()
            .map(|raw| (leading_columns(raw), hard_break(raw.trim()).0))
            .collect();
        let levels = verse_levels(&texts);
        let mut verse: Vec<Vec<Inline>> = Vec::new();
        let mut indents: Vec<u8> = Vec::new();
        for (&(_, text), level) in texts.iter().zip(levels) {
            if !text.is_empty() {
                let content = self.inlines(text);
                verse.push(content);
                indents.push(level);
            } else if verse.last().is_some_and(|l| !l.is_empty()) {
                verse.push(Vec::new());
                indents.push(0);
            }
        }
        while verse.last().is_some_and(Vec::is_empty) {
            verse.pop();
            indents.pop();
        }
        if !verse.is_empty() {
            self.push(Block::verse(verse).with_indents(indents), line_no);
        }
        if !closed {
            self.fence_not_closed("verse", line_no);
        }
        next
    }

    /// The epigraph fence opening at `lines[start]`; returns the index of the
    /// first line after it. Its text is joined like a paragraph (blank lines
    /// become line breaks: the model holds one run of text); a last line
    /// starting with a dash is the attribution, as plain text. Right after a
    /// heading it is that section's epigraph, before the first heading an
    /// `Epigraph` section of its own; anywhere else it is a block quote with
    /// the attribution line as its last paragraph.
    fn epigraph(&mut self, lines: &[&str], start: usize) -> usize {
        let line_no = start + 1;
        let (inner, next, closed) = fence_body(lines, start);
        let inner: Vec<&str> = inner.into_iter().map(str::trim).collect();
        let mut paras: Vec<Vec<&str>> = inner
            .split(|l| l.is_empty())
            .filter(|p| !p.is_empty())
            .map(<[&str]>::to_vec)
            .collect();
        let mut attribution = None;
        if paras.iter().map(Vec::len).sum::<usize>() > 1
            && let Some(last) = paras.last_mut()
            && let Some(line) = last.pop_if(|l| crate::attribution_text(l).is_some())
        {
            attribution = Some(line);
            if last.is_empty() {
                paras.pop();
            }
        }
        // `None` before the first heading, `Some(true)` right after one.
        let at_heading = self
            .current()
            .map(|s| s.blocks.is_empty() && s.epigraph.is_none());
        if at_heading == Some(false) {
            paras.extend(attribution.map(|a| vec![a]));
            let blocks: Vec<Block> = paras
                .iter()
                .map(|p| Block::paragraph(self.inlines(&join_lines(p))))
                .collect();
            if !blocks.is_empty() {
                self.push(Block::block_quote(blocks), line_no);
            }
        } else if !paras.is_empty() {
            let mut text = Vec::new();
            for (n, p) in paras.iter().enumerate() {
                if n > 0 {
                    text.push(Inline::LineBreak);
                }
                text.extend(self.inlines(&join_lines(p)));
            }
            let attribution = attribution.and_then(crate::attribution_text).map(|a| {
                let inlines = parse_inlines(a, &mut Notes::default());
                Inline::plain_text(&inlines).trim().to_string()
            });
            let epigraph = Epigraph { text, attribution };
            match self.current() {
                Some(s) => s.epigraph = Some(epigraph),
                None => {
                    self.sections.push(Section {
                        style: Default::default(),
                        kind: SectionKind::Epigraph,
                        title: None,
                        numbering: Numbering::None,
                        epigraph: Some(epigraph),
                        blocks: Vec::new(),
                    });
                    self.front = self.sections.len();
                }
            }
            self.note_warnings(line_no);
        }
        if !closed {
            self.fence_not_closed("epigraph", line_no);
        }
        next
    }

    /// Warn about a fence without its `:::` (see [`fence_body`]).
    fn fence_not_closed(&mut self, name: &str, line_no: usize) {
        self.warnings.push(ImportWarning {
            kind: WarningKind::VerseNotClosed,
            location: line_no,
            section: self.current_index(),
            detail: Some(WarningDetail::Fence { name: name.into() }),
            message: format!(
                "This `:::{name}` has no closing `:::` line, so it was ended before the next \
                 heading. Add `:::` where the {name} ends."
            ),
        });
    }

    /// Warn about footnote marks without a definition in the block just
    /// added, which started on `line_no`.
    fn note_warnings(&mut self, line_no: usize) {
        let section = self.sections.len().checked_sub(1);
        for label in self.notes.missing.drain(..) {
            self.warnings.push(ImportWarning {
                kind: WarningKind::FootnoteMissing,
                location: line_no,
                section,
                detail: Some(WarningDetail::Footnote {
                    label: label.clone(),
                }),
                message: format!(
                    "Footnote [^{label}] has no `[^{label}]: …` line; the mark was kept as text."
                ),
            });
        }
    }

    /// Warn about definitions no mark refers to: their text is not in the
    /// book, so the author must hear about it.
    fn unused_note_warnings(&mut self) {
        let mut unused: Vec<(usize, String)> = self
            .notes
            .defs
            .iter()
            .filter(|(_, d)| !d.used)
            .map(|(label, d)| (d.line, label.clone()))
            .collect();
        unused.sort();
        for (line, label) in unused {
            self.warnings.push(unused_note(line, &label, ""));
        }
        for (label, line) in std::mem::take(&mut self.notes.duplicates) {
            self.warnings.push(unused_note(line, &label, " again"));
        }
        self.warnings.sort_by_key(|w| w.location);
    }
}

fn unused_note(line: usize, label: &str, again: &str) -> ImportWarning {
    ImportWarning {
        kind: WarningKind::FootnoteUnused,
        location: line,
        section: None,
        detail: Some(WarningDetail::Footnote {
            label: label.into(),
        }),
        message: format!(
            "Footnote [^{label}] is defined{again} here, but no [^{label}] mark uses this text; \
             it is not in the book."
        ),
    }
}

/// Footnote definitions, taken out of the text before the body is read:
///
/// ```text
/// [^1]: The note text, continued on the
/// next line.
///
///     An indented paragraph (tab or 4 spaces) after a blank line
///     belongs to the same note.
/// ```
///
/// The first definition of a label wins. A mark inside a note is kept as
/// text: notes on notes do not exist in fiction.
#[derive(Default)]
struct Notes {
    defs: HashMap<String, NoteDef>,
    /// Labels defined a second time, with the line of that definition.
    duplicates: Vec<(String, usize)>,
    /// Marks read since the last [`Body::note_warnings`] that have no
    /// definition.
    missing: Vec<String>,
}

struct NoteDef {
    line: usize,
    paras: Vec<String>,
    used: bool,
}

impl Notes {
    /// Take the definitions out of `lines` (which start at line index
    /// `offset`), leaving blank lines so line numbers stay right.
    fn collect(lines: &mut [&str], offset: usize) -> Self {
        let mut notes = Notes::default();
        let mut i = 0;
        while i < lines.len() {
            let Some((label, first)) = note_definition(lines[i]) else {
                i += 1;
                continue;
            };
            let line = offset + i + 1;
            let mut paras = vec![vec![first.trim()]];
            lines[i] = "";
            i += 1;
            while i < lines.len() {
                let raw = lines[i];
                let next = raw.trim();
                let in_para = paras
                    .last()
                    .is_some_and(|p| p.iter().any(|l| !l.is_empty()));
                if next.is_empty() {
                    // A blank line ends the note unless an indented line
                    // follows it.
                    let indented = lines
                        .get(i + 1)
                        .is_some_and(|l| is_indented(l) && !l.trim().is_empty());
                    if !indented {
                        break;
                    }
                    paras.push(Vec::new());
                } else if is_indented(raw) || (in_para && !ends_note(next)) {
                    if let Some(p) = paras.last_mut() {
                        p.push(next);
                    }
                } else {
                    break;
                }
                lines[i] = "";
                i += 1;
            }
            let paras: Vec<String> = paras
                .iter()
                .map(|p| join_lines(p).trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if notes.defs.contains_key(label) {
                notes.duplicates.push((label.to_string(), line));
            } else {
                notes.defs.insert(
                    label.to_string(),
                    NoteDef {
                        line,
                        paras,
                        used: false,
                    },
                );
            }
        }
        notes
    }

    /// The note for a mark, or `None` (recorded as missing).
    fn resolve(&mut self, label: &str) -> Option<Vec<Block>> {
        let Some(def) = self.defs.get_mut(label) else {
            self.missing.push(label.to_string());
            return None;
        };
        def.used = true;
        let mut inner = Notes::default();
        Some(
            def.paras
                .iter()
                .map(|p| Block::paragraph(parse_inlines(p, &mut inner)))
                .collect(),
        )
    }
}

/// `[^label]: text` → (label, text).
fn note_definition(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix("[^")?;
    let (label, text) = rest.split_once("]:")?;
    is_note_label(label).then_some((label, text))
}

fn is_note_label(label: &str) -> bool {
    !label.is_empty()
        && label.chars().count() <= 40
        && !label.contains(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '^'))
}

fn is_indented(line: &str) -> bool {
    line.starts_with('\t') || line.starts_with("    ")
}

/// Whether a line after a note definition is not part of the note.
fn ends_note(line: &str) -> bool {
    starts_block(line) || line.starts_with('>') || note_definition(line).is_some()
}

/// A `#` heading's text without a trailing Pandoc `{-}` or `{.unnumbered}`,
/// and whether it had one.
fn unnumbered(text: &str) -> (&str, bool) {
    let t = text.trim_end();
    for mark in ["{-}", "{.unnumbered}"] {
        if let Some(rest) = t.strip_suffix(mark) {
            return (rest.trim_end(), true);
        }
    }
    (text, false)
}

/// `## Text` … `###### Text` → (level, text). The level counts from the
/// section title (`#`), so `##` is level 1, as for .docx sub-headings.
/// Closing hashes (`## Text ##`) are dropped. A line of hashes alone is a
/// scene break, and `#######` or `##Text` are paragraph text.
fn sub_heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(2..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let mut text = rest.trim();
    let unclosed = text.trim_end_matches('#');
    if unclosed.is_empty() || unclosed.ends_with([' ', '\t']) {
        text = unclosed.trim_end();
    }
    if text.is_empty() {
        return None;
    }
    Some((hashes as u8 - 1, text))
}

/// The quote starting at `lines[start]` (a `>` line) and the index of the
/// first line after it. Consecutive `>` lines join into paragraphs; a bare
/// `>` separates them. A text line right after quoted text continues it
/// (lazy continuation: hard-wrapped manuscripts often mark only the first
/// line). Nested `> >` is flattened; fiction has no use for quotes in quotes.
fn block_quote(lines: &[&str], start: usize, notes: &mut Notes) -> (Option<Block>, usize) {
    let mut paras: Vec<Vec<&str>> = vec![Vec::new()];
    let mut i = start;
    while let Some(raw) = lines.get(i) {
        let line = raw.trim();
        let in_para = paras.last().is_some_and(|p| !p.is_empty());
        let text = if line.starts_with('>') {
            unquote(line)
        } else if !line.is_empty() && in_para && !starts_block(line) {
            line
        } else {
            break;
        };
        if text.is_empty() {
            if in_para {
                paras.push(Vec::new());
            }
        } else if let Some(p) = paras.last_mut() {
            p.push(text);
        }
        i += 1;
    }
    let blocks: Vec<Block> = paras
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| Block::paragraph(parse_inlines(&join_lines(p), notes)))
        .collect();
    ((!blocks.is_empty()).then(|| Block::block_quote(blocks)), i)
}

/// A quoted line without its `>` markers (all of them, for nested quotes).
fn unquote(line: &str) -> &str {
    let mut s = line;
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s
}

/// Whether a line starts a block of its own instead of continuing a quote.
fn starts_block(line: &str) -> bool {
    line.starts_with("# ")
        || sub_heading(line).is_some()
        || heading::is_scene_break(line)
        || fence(line).is_some()
        || is_page_break(line)
        || image_line(line).is_some()
}

/// A list item line, `- text` (also `*`, `+`) or `1. text` (also `1)`).
struct ItemLine<'s> {
    /// Width of the leading whitespace, a tab counting 4.
    indent: usize,
    /// The number of an ordered item.
    number: Option<u32>,
    text: &'s str,
}

/// `raw` (untrimmed, for its indentation) as a list item. An item needs a
/// space after its marker and some text; `* * *` is a scene break.
fn item_line(raw: &str) -> Option<ItemLine<'_>> {
    let line = raw.trim();
    if heading::is_scene_break(line) {
        return None;
    }
    let (number, rest) = match line.strip_prefix(['-', '*', '+']) {
        Some(rest) => (None, rest),
        None => {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            if !(1..=3).contains(&digits) {
                return None;
            }
            let rest = line[digits..].strip_prefix(['.', ')'])?;
            (line[..digits].parse().ok(), rest)
        }
    };
    if !rest.starts_with([' ', '\t']) || rest.trim().is_empty() {
        return None;
    }
    Some(ItemLine {
        indent: indent_width(raw),
        number,
        text: rest.trim(),
    })
}

fn indent_width(raw: &str) -> usize {
    raw.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// One list item as written: its marker and the lines of its paragraphs.
struct RawItem<'s> {
    /// Index of the item's first line, which `paras[0][0]` is without
    /// its marker.
    line: usize,
    indent: usize,
    number: Option<u32>,
    paras: Vec<Vec<&'s str>>,
}

/// What [`list`] found.
enum ListRun<'s> {
    Lists(Vec<Block>),
    /// An ordered list that starts at `start`, not 1: its items as
    /// paragraphs (line index, lines, numbers kept).
    Numbered {
        start: u32,
        paras: Vec<(usize, Vec<&'s str>)>,
    },
}

/// The lists starting at `lines[start]` and the index of the first line
/// after them, or `None` if `lines[start]` does not start a list. Item text
/// continues on the lines below it (lazily, as in quotes); after a blank
/// line, another item continues the list and an indented line (2 spaces or
/// a tab) starts a second paragraph of the item. An item indented deeper
/// than the one before it starts a list inside that one. An ordered list
/// starts at 1 and has a second item: the model numbers from 1, and a
/// paragraph opening with a date ("3. Oktober", "1. Mai war …") stays a
/// paragraph. Items that count on from another number ("8.", "9.": a list
/// resumed after a paragraph) are [`ListRun::Numbered`], so they do not run
/// together; the next number is required, so a wrapped line starting with
/// a date ("24. Dezember") still continues a paragraph that opens with one.
fn list<'s>(lines: &[&'s str], start: usize, notes: &mut Notes) -> Option<(ListRun<'s>, usize)> {
    item_line(lines[start])?;
    let mut items: Vec<RawItem> = Vec::new();
    let mut i = start;
    while let Some(&raw) = lines.get(i) {
        let line = raw.trim();
        if let Some(item) = item_line(raw) {
            items.push(RawItem {
                line: i,
                indent: item.indent,
                number: item.number,
                paras: vec![vec![item.text]],
            });
        } else if line.is_empty() {
            let next = lines[i..].iter().find(|l| !l.trim().is_empty());
            match next {
                Some(n) if item_line(n).is_some() => {}
                Some(n) if indent_width(n) >= 2 && !ends_item(n.trim()) => {
                    if let Some(item) = items.last_mut()
                        && item.paras.last().is_some_and(|p| !p.is_empty())
                    {
                        item.paras.push(Vec::new());
                    }
                }
                _ => break,
            }
        } else if ends_item(line) {
            break;
        } else if let Some(p) = items.last_mut().and_then(|it| it.paras.last_mut()) {
            p.push(line);
        }
        i += 1;
    }
    let first = items.first()?;
    if let Some(n) = first.number {
        let second = items[1..].iter().find(|it| it.indent <= first.indent);
        match second.and_then(|it| it.number) {
            Some(_) if n == 1 => {}
            Some(m) if m == n + 1 => {
                // The run ends at the first item at its depth that does not
                // count on; the main loop reads on from there.
                let mut expected = n;
                let end = items.iter().position(|it| {
                    if it.indent > first.indent {
                        return false;
                    }
                    let counts = it.number == Some(expected);
                    expected += 1;
                    !counts
                });
                let (run, next) = match end {
                    Some(k) => (&items[..k], items[k].line),
                    None => (&items[..], i),
                };
                let paras = numbered_paras(lines, run);
                return Some((ListRun::Numbered { start: n, paras }, next));
            }
            _ => return None,
        }
    }
    Some((ListRun::Lists(build_lists(&items, notes)), i))
}

/// The items as paragraphs (line index, lines), each first line with its
/// marker as written.
fn numbered_paras<'s>(lines: &[&'s str], items: &[RawItem<'s>]) -> Vec<(usize, Vec<&'s str>)> {
    let mut paras = Vec::new();
    for item in items {
        for (n, p) in item.paras.iter().filter(|p| !p.is_empty()).enumerate() {
            let mut p = p.clone();
            if n == 0
                && let Some(first) = p.first_mut()
            {
                *first = lines[item.line].trim();
            }
            paras.push((item.line, p));
        }
    }
    paras
}

/// Whether a line ends a list item instead of continuing its text.
fn ends_item(line: &str) -> bool {
    starts_block(line) || line.starts_with('>')
}

/// Nest `items` by indentation: the items after one that are indented
/// deeper form a list inside it. A change between ordered and unordered
/// items at the same depth starts a new list.
fn build_lists(items: &[RawItem], notes: &mut Notes) -> Vec<Block> {
    let mut lists = Vec::new();
    let mut i = 0;
    while let Some(first) = items.get(i) {
        let ordered = first.number.is_some();
        let depth = first.indent;
        let mut list_items = Vec::new();
        while let Some(item) = items.get(i).filter(|it| it.number.is_some() == ordered) {
            let mut blocks: Vec<Block> = item
                .paras
                .iter()
                .filter(|p| !p.is_empty())
                .map(|p| Block::paragraph(parse_inlines(&join_lines(p), notes)))
                .collect();
            let end = items[i + 1..]
                .iter()
                .position(|it| it.indent <= depth)
                .map_or(items.len(), |n| i + 1 + n);
            blocks.extend(build_lists(&items[i + 1..end], notes));
            list_items.push(blocks);
            i = end;
        }
        lists.push(Block::list(ordered, list_items));
    }
    lists
}

/// Most paragraphs in a dedication before the first heading, and most
/// characters in all of them. Shorter than a .docx dedication page (600):
/// Markdown has no centring or page to go by.
const DEDICATION_PARAS: usize = 4;
const DEDICATION_CHARS: usize = 300;

/// A table row: its line index and its cells.
type TableRow = (usize, Vec<String>);

/// A pipe table as written: each column's alignment (from the delimiter
/// row, which is never text) and the rows, the header row first.
struct PipeTable {
    aligns: Vec<ColumnAlign>,
    rows: Vec<TableRow>,
}

/// A Pandoc table caption, the text of a `Table: text` line.
#[derive(Clone, Copy)]
struct Caption<'a> {
    text: &'a str,
    /// Written above the table (else below it).
    above: bool,
    /// The index of the caption's line.
    line: usize,
}

/// The text of a `Table: text` line, if it is one.
fn caption_text(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("Table:")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some(rest.trim()).filter(|text| !text.is_empty())
}

/// A caption above a table: the `Table: text` line at `lines[i]`, then the
/// table, directly or after one blank line. Returns the caption, the table
/// and the index after the table. The line has to start a paragraph (the
/// caller checks), so prose cannot turn into a caption by accident.
fn caption_above<'a>(lines: &[&'a str], i: usize) -> Option<(Caption<'a>, PipeTable, usize)> {
    let text = caption_text(lines[i])?;
    let at = match lines.get(i + 1) {
        Some(next) if next.trim().is_empty() => i + 2,
        _ => i + 1,
    };
    let (pipe_table, next) = table(lines, at)?;
    Some((
        Caption {
            text,
            above: true,
            line: i,
        },
        pipe_table,
        next,
    ))
}

/// A caption below a table that ends before line `at`: a `Table: text` line
/// right there or after one blank line, alone in its paragraph (a blank line,
/// the end or a line that starts a block of its own after it) and not the
/// caption of the table that follows it.
/// Returns the caption and the index after its line.
fn caption_below<'a>(lines: &[&'a str], at: usize) -> Option<(Caption<'a>, usize)> {
    let at = match lines.get(at) {
        Some(line) if line.trim().is_empty() => at + 1,
        _ => at,
    };
    let text = caption_text(lines.get(at)?)?;
    match lines.get(at + 1) {
        None => {}
        Some(next) if next.trim().is_empty() && table(lines, at + 2).is_none() => {}
        Some(next) if starts_block(next) || next.starts_with('>') => {}
        Some(_) => return None,
    }
    Some((
        Caption {
            text,
            above: false,
            line: at,
        },
        at + 1,
    ))
}

/// The alignment a delimiter cell (`:--`, `--:`, `:-:`, `---`) says, or
/// `None` when the cell is not a delimiter.
fn delimiter_align(cell: &str) -> Option<ColumnAlign> {
    let left = cell.starts_with(':');
    let right = cell.ends_with(':');
    let dashes = cell.strip_prefix(':').unwrap_or(cell);
    let dashes = dashes.strip_suffix(':').unwrap_or(dashes);
    if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
        return None;
    }
    Some(match (left, right) {
        (true, true) => ColumnAlign::Center,
        (false, true) => ColumnAlign::Right,
        _ => ColumnAlign::Left,
    })
}

/// The pipe table (GFM) starting at `lines[start]` and the index of the
/// first line after it, or `None`. A table is a header row, a delimiter row
/// with as many cells (`|:--|--:|`, never text), then rows up to a blank
/// line, a line without `|` or another block. It may follow a paragraph line
/// directly: nothing in prose looks like a delimiter row.
fn table(lines: &[&str], start: usize) -> Option<(PipeTable, usize)> {
    let header = lines.get(start)?.trim();
    if !is_table_row(header) {
        return None;
    }
    let delimiter = lines.get(start + 1)?.trim();
    let cells = table_cells(header);
    let aligns: Vec<ColumnAlign> = table_cells(delimiter)
        .iter()
        .map(|c| delimiter_align(c))
        .collect::<Option<_>>()?;
    if !is_table_row(delimiter) || aligns.is_empty() || aligns.len() != cells.len() {
        return None;
    }
    let mut rows = vec![(start, cells)];
    let mut i = start + 2;
    while let Some(line) = lines.get(i).map(|l| l.trim()) {
        if !is_table_row(line) || starts_block(line) || line.starts_with('>') {
            break;
        }
        rows.push((i, table_cells(line)));
        i += 1;
    }
    Some((PipeTable { aligns, rows }, i))
}

/// Whether a line has a `|` that is not escaped (`\|`).
fn is_table_row(line: &str) -> bool {
    line.char_indices()
        .any(|(i, c)| c == '|' && !line[..i].ends_with('\\'))
}

/// A table row's cells, trimmed, without the empty ones the outer pipes
/// make; `\|` is a `|` in a cell.
fn table_cells(line: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let escaped = c == '\\' && chars.peek() == Some(&'|');
        if c == '|' {
            cells.push(String::new());
        } else if let Some(cell) = cells.last_mut() {
            cell.push(if escaped {
                chars.next().unwrap_or(c)
            } else {
                c
            });
        }
    }
    if line.starts_with('|') {
        cells.remove(0);
    }
    if line.ends_with('|') && !line.ends_with("\\|") {
        cells.pop();
    }
    cells.iter().map(|c| c.trim().to_string()).collect()
}

/// An image line, `![alt](path "caption")`, split into its parts.
struct ImageLine<'s> {
    alt: &'s str,
    path: &'s str,
    caption: Option<&'s str>,
    /// The text inside a trailing `{…}`, empty when there is none.
    attrs: &'s str,
}

/// A line that is one image and nothing else. The path may be wrapped in
/// `<…>` and may contain spaces (authors write `my map.png`); a Pandoc
/// attribute block after it (`{width=50%}`, `{.beside .left}`) is read by
/// `image_layout`.
fn image_line(line: &str) -> Option<ImageLine<'_>> {
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let (rest, attrs) = match rest.rfind("){") {
        Some(end) if rest.ends_with('}') => (&rest[..=end], &rest[end + 2..rest.len() - 1]),
        _ => (rest, ""),
    };
    let inner = rest.strip_suffix(')')?;
    let (path, caption) = match inner.trim_end().strip_suffix('"') {
        Some(body) => match body.split_once('"') {
            Some((p, c)) if p.ends_with([' ', '\t']) => (p, Some(c)),
            _ => (inner, None),
        },
        None => (inner, None),
    };
    let path = path.trim();
    let path = path
        .strip_prefix('<')
        .and_then(|p| p.strip_suffix('>'))
        .unwrap_or(path)
        .trim();
    (!path.is_empty()).then_some(ImageLine {
        alt,
        path,
        caption,
        attrs,
    })
}

/// A picture's size and placement from the attributes after it. Reads
/// `width=50%` (or `width=50`; rounded to 5, clamped to 10–100; anything
/// else is ignored), `.beside` with `.outside`, `.inside`, `.left` or
/// `.right`, `.page` and `.bleed`; other attributes are ignored. When
/// several placements are named the widest claim wins: bleed, page, beside.
/// A side without `.beside` and a width on a bleed picture have no effect
/// and are not kept.
fn image_layout(attrs: &str) -> (Option<u8>, ImagePlacement, ImageSide) {
    let (mut width, mut side) = (None, ImageSide::Outside);
    let (mut beside, mut page, mut bleed) = (false, false, false);
    for attr in attrs.split_whitespace() {
        match attr {
            ".beside" => beside = true,
            ".page" => page = true,
            ".bleed" => bleed = true,
            ".outside" => side = ImageSide::Outside,
            ".inside" => side = ImageSide::Inside,
            ".left" => side = ImageSide::Left,
            ".right" => side = ImageSide::Right,
            _ => {
                if let Some(v) = attr.strip_prefix("width=") {
                    let v = v.trim_matches(['"', '\'']);
                    let v = v.strip_suffix('%').unwrap_or(v);
                    if let Ok(pct) = v.parse::<f64>()
                        && pct.is_finite()
                    {
                        let rounded = (pct / 5.0).round() * 5.0;
                        width = Some(rounded.clamp(10.0, 100.0) as u8);
                    }
                }
            }
        }
    }
    let placement = if bleed {
        ImagePlacement::Bleed
    } else if page {
        ImagePlacement::Page
    } else if beside {
        ImagePlacement::Beside
    } else {
        ImagePlacement::Inline
    };
    if placement == ImagePlacement::Bleed {
        width = None;
    }
    if placement != ImagePlacement::Beside {
        side = ImageSide::Outside;
    }
    (width, placement, side)
}

/// What a fenced div holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Verse,
    Epigraph,
}

/// `:::verse`, also `::: verse`, `:::: Verse ::::` (Pandoc-style fenced
/// div), or the same with `epigraph`.
fn fence(line: &str) -> Option<Fence> {
    let colons = line.bytes().take_while(|&b| b == b':').count();
    if colons < 3 {
        return None;
    }
    let name = line[colons..].trim_end_matches(':').trim();
    if name.eq_ignore_ascii_case("verse") {
        Some(Fence::Verse)
    } else if name.eq_ignore_ascii_case("epigraph") {
        Some(Fence::Epigraph)
    } else {
        None
    }
}

/// The lines (as written, not trimmed) inside the fence opening at
/// `lines[start]`, the index of the first line after it, and whether its
/// `:::` was found. A fence without its `:::` ends before the next heading,
/// or at the end (the caller warns): the rest of the book would otherwise be
/// read into it.
fn fence_body<'s>(lines: &[&'s str], start: usize) -> (Vec<&'s str>, usize, bool) {
    let mut inner = Vec::new();
    let mut i = start + 1;
    while let Some(raw) = lines.get(i) {
        let line = raw.trim();
        if is_fence_end(line) {
            return (inner, i + 1, true);
        }
        if line.starts_with("# ") || sub_heading(line).is_some() {
            break;
        }
        inner.push(*raw);
        i += 1;
    }
    (inner, i, false)
}

/// Columns a verse line is indented by: a space is one, a tab four (so a tab
/// is one level), any other white space one.
fn leading_columns(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Spaces that make one verse indent level. Four, not two: an editor that
/// keeps the previous line's indent, or a hand-wrapped line, leaves one or
/// two stray spaces that must not become an indent; four is a tab's width
/// and what Gutenberg's plain text uses for Carroll's verse.
const SPACES_PER_LEVEL: usize = 4;

/// The indent level of each `(leading columns, text)` verse line: the
/// columns beyond the least-indented non-blank line (a verse typed with all
/// its lines indented is flush, as in a code block), one level per
/// [`SPACES_PER_LEVEL`], at most [`MAX_VERSE_INDENT`]. Blank lines are 0.
fn verse_levels(lines: &[(usize, &str)]) -> Vec<u8> {
    let least = lines
        .iter()
        .filter(|(_, text)| !text.is_empty())
        .map(|&(cols, _)| cols)
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|&(cols, text)| {
            if text.is_empty() {
                return 0;
            }
            let level = (cols - least) / SPACES_PER_LEVEL;
            u8::try_from(level).map_or(MAX_VERSE_INDENT, |l| l.min(MAX_VERSE_INDENT))
        })
        .collect()
}

/// `\newpage` or `\pagebreak` alone on a line: LaTeX, as Pandoc users write
/// it (Pandoc's page-break filters read it too).
fn is_page_break(line: &str) -> bool {
    matches!(line, "\\newpage" | "\\pagebreak")
}

/// `:::` (or more colons) alone closes a verse fence.
fn is_fence_end(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b':')
}

/// A line without its trailing `\` (a hard line break), and whether it had
/// one.
fn hard_break(line: &str) -> (&str, bool) {
    match line.strip_suffix('\\') {
        Some(text) => (text.trim_end(), true),
        None => (line, false),
    }
}

/// The lines of one paragraph joined with spaces; after a line ending in
/// `\` comes a `\n` instead, which [`parse_inlines`] makes a line break.
/// On the last line the `\` is dropped: there is nothing to break before.
fn join_lines(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut sep = "";
    for line in lines {
        let (text, brk) = hard_break(line);
        out.push_str(sep);
        out.push_str(text);
        sep = if brk { "\n" } else { " " };
    }
    out
}

/// Parse `*em*`, `_em_`, `**strong**`, `[small caps]{.smallcaps}`, `^sup^`,
/// `~sub~`, links `[text](url)` and `<https://…>`, footnote marks `[^label]`
/// and line breaks (`\n`, from [`join_lines`]). Unbalanced markers and marks
/// without a definition stay literal.
fn parse_inlines(s: &str, notes: &mut Notes) -> Vec<Inline> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            flush_text(&mut buf, &mut out);
            out.push(Inline::LineBreak);
            i += 1;
            continue;
        }
        if c == '['
            && chars.get(i + 1) == Some(&'^')
            && let Some(len) = chars[i + 2..].iter().position(|&c| c == ']')
        {
            let label: String = chars[i + 2..i + 2 + len].iter().collect();
            if is_note_label(&label)
                && let Some(blocks) = notes.resolve(&label)
            {
                // The mark sits on the word: a space typed before it would
                // separate the superscript from the word.
                let kept = buf.trim_end().len();
                buf.truncate(kept);
                flush_text(&mut buf, &mut out);
                out.push(Inline::Footnote { blocks });
                i += len + 3;
                continue;
            }
        }
        // A Pandoc span, `[text]{.smallcaps}`; other attributes are dropped
        // and the text kept.
        if opens_bracket(&chars, i)
            && let Some(close) = closing_bracket(&chars, i)
            && chars.get(close + 1) == Some(&'{')
            && let Some(len) = chars[close + 2..].iter().position(|&c| c == '}')
        {
            let attrs: String = chars[close + 2..close + 2 + len].iter().collect();
            let inner: String = chars[i + 1..close].iter().collect();
            let content = parse_inlines(&inner, notes);
            let small_caps = attrs
                .split_whitespace()
                .any(|a| a.eq_ignore_ascii_case(".smallcaps"));
            if small_caps && !content.is_empty() {
                flush_text(&mut buf, &mut out);
                out.push(Inline::SmallCaps { content });
            } else {
                for inline in content {
                    match inline {
                        Inline::Text { text } => buf.push_str(&text),
                        other => {
                            flush_text(&mut buf, &mut out);
                            out.push(other);
                        }
                    }
                }
            }
            i = close + len + 3;
            continue;
        }
        // A link, `[text](url "title")`; the title is dropped.
        if opens_bracket(&chars, i)
            && let Some(close) = closing_bracket(&chars, i)
            && chars.get(close + 1) == Some(&'(')
            && let Some(end) = closing_bracket(&chars, close + 1)
            && let Some(url) = link_url(&chars[close + 2..end].iter().collect::<String>())
        {
            let inner: String = chars[i + 1..close].iter().collect();
            let mut content = parse_inlines(&inner, notes);
            if content.is_empty() {
                content.push(Inline::text(url.clone()));
            }
            flush_text(&mut buf, &mut out);
            out.push(Inline::Link { url, content });
            i = end + 1;
            continue;
        }
        // An autolink, `<https://…>` or `<mailto:…>`.
        if c == '<'
            && let Some(len) = chars[i + 1..].iter().position(|&c| c == '>')
        {
            let url: String = chars[i + 1..i + 1 + len].iter().collect();
            if is_autolink(&url) {
                let text = url.strip_prefix("mailto:").unwrap_or(&url).to_string();
                flush_text(&mut buf, &mut out);
                out.push(Inline::Link {
                    url,
                    content: vec![Inline::text(text)],
                });
                i += len + 2;
                continue;
            }
        }
        // `^sup^` and `~sub~` (Pandoc); doubled markers stay text.
        if c == '^' || c == '~' {
            if chars.get(i + 1) == Some(&c) {
                buf.extend([c, c]);
                i += 2;
                continue;
            }
            if let Some(end) = script_end(&chars, i) {
                flush_text(&mut buf, &mut out);
                let inner: String = chars[i + 1..end].iter().collect();
                let content = parse_inlines(&inner, notes);
                out.push(if c == '^' {
                    Inline::Superscript { content }
                } else {
                    Inline::Subscript { content }
                });
                i = end + 1;
                continue;
            }
        }
        let strong = c == '*' && chars.get(i + 1) == Some(&'*');
        if strong {
            if let Some(end) = find_closing(&chars, i + 2, &['*', '*']) {
                flush_text(&mut buf, &mut out);
                let inner: String = chars[i + 2..end].iter().collect();
                out.push(Inline::Strong {
                    content: parse_inlines(&inner, notes),
                });
                i = end + 2;
                continue;
            }
        } else if (c == '*' || c == '_')
            && opens_emphasis(&chars, i)
            && let Some(end) = find_closing(&chars, i + 1, &[c])
        {
            flush_text(&mut buf, &mut out);
            let inner: String = chars[i + 1..end].iter().collect();
            out.push(Inline::Emphasis {
                content: parse_inlines(&inner, notes),
            });
            i = end + 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush_text(&mut buf, &mut out);
    out
}

/// The index of the marker that closes `^…^` or `~…~` opened at `open`.
/// The text between is not empty and has no whitespace, so `~50 km` and
/// `a ~ b` stay text.
fn script_end(chars: &[char], open: usize) -> Option<usize> {
    let marker = chars[open];
    let len = chars[open + 1..]
        .iter()
        .position(|&c| c == marker || c.is_whitespace())?;
    (len > 0 && chars[open + 1 + len] == marker).then_some(open + 1 + len)
}

/// Whether the `[` at `i` can open a span or a link: not a footnote mark
/// `[^…]`, and not `![…]` (an image inside a sentence stays text).
fn opens_bracket(chars: &[char], i: usize) -> bool {
    chars[i] == '[' && chars.get(i + 1) != Some(&'^') && (i == 0 || chars[i - 1] != '!')
}

/// The index of the `]` or `)` that closes the `[` or `(` at `open`, past
/// nested pairs (`[a [b]]`, `(https://…/Rope_(knot))`).
fn closing_bracket(chars: &[char], open: usize) -> Option<usize> {
    let (opening, closing) = if chars[open] == '(' {
        ('(', ')')
    } else {
        ('[', ']')
    };
    let mut depth = 0usize;
    for (j, &c) in chars.iter().enumerate().skip(open) {
        if c == opening {
            depth += 1;
        } else if c == closing {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

/// The URL of a link target, `url "title"` or `<url with spaces>`, or
/// `None` if there is none.
fn link_url(target: &str) -> Option<String> {
    let target = target.trim();
    let url = match target.strip_prefix('<') {
        Some(rest) => rest.split_once('>')?.0.trim(),
        None => target.split_whitespace().next()?,
    };
    (!url.is_empty()).then(|| url.to_string())
}

/// `https://…`, `http://…` or `mailto:…` with no whitespace.
fn is_autolink(url: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|p| url.len() > p.len() && url.starts_with(p))
        && !url.contains(char::is_whitespace)
}

fn opens_emphasis(chars: &[char], i: usize) -> bool {
    // Must be followed by a non-space and, for `_`, not inside a word.
    let next_ok = chars.get(i + 1).is_some_and(|n| !n.is_whitespace());
    let prev_ok = i == 0 || !chars[i - 1].is_alphanumeric() || chars[i] == '*';
    next_ok && prev_ok
}

fn find_closing(chars: &[char], from: usize, marker: &[char]) -> Option<usize> {
    let n = marker.len();
    let mut j = from;
    while j + n <= chars.len() {
        // For single `*`, a `**` is strong inside the emphasis: skip both
        // stars, wherever it stands (`*a **b** c*`).
        if n == 1 && marker[0] == '*' && chars[j] == '*' && chars.get(j + 1) == Some(&'*') {
            j += 2;
            continue;
        }
        if chars[j..j + n] == *marker && j > from && !chars[j - 1].is_whitespace() {
            return Some(j);
        }
        j += 1;
    }
    None
}

fn flush_text(buf: &mut String, out: &mut Vec<Inline>) {
    if !buf.is_empty() {
        out.push(Inline::Text {
            text: std::mem::take(buf),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Import without any image files.
    fn import(src: &str) -> Result<Imported, ImportError> {
        super::import(src, |_| None)
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    const SAMPLE: &str = "Title: Ein Landarzt\nAuthor: Franz Kafka\nLang: de\nDedication: Für M.\n\n# Prolog\n\nIch war in *großer* Verlegenheit:\neine dringende Reise stand mir bevor.\n\n***\n\nDanach.\n\n# Auf der Galerie\n\nErster **Absatz**.\n\n* * *\n";

    #[test]
    fn imports_structure() {
        let Imported { book, warnings, .. } = import(SAMPLE).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(book.metadata.title, "Ein Landarzt");
        assert_eq!(book.metadata.lang.primary(), "de");
        let kinds: Vec<_> = book.sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SectionKind::Dedication,
                SectionKind::Prologue,
                SectionKind::Chapter
            ]
        );
        let prolog = &book.sections[1];
        assert_eq!(prolog.blocks.len(), 3);
        assert_eq!(prolog.blocks[1], Block::scene_break());
        // Lines of one paragraph are joined with a space.
        let Block::Paragraph { content, .. } = &prolog.blocks[0] else {
            panic!()
        };
        assert_eq!(
            Inline::plain_text(content),
            "Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor."
        );
        // Trailing scene break removed.
        assert_eq!(book.sections[2].blocks.len(), 1);
    }

    #[test]
    fn text_before_first_chapter_stays_out_of_the_dedication() {
        let src = "Title: T\nDedication: Für A.\nGenre: Roman\n\n***\n\n# Eins\n\nB.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let kinds: Vec<_> = book.sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::Dedication,
                SectionKind::Other,
                SectionKind::Chapter
            ]
        );
        assert_eq!(book.sections[0].blocks.len(), 1);
        assert_eq!(
            book.sections[1].blocks,
            vec![Block::paragraph(vec![Inline::text("Genre: Roman")])]
        );
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        assert_eq!(got, [(WarningKind::TextBeforeFirstChapter, 3, Some(1))]);
    }

    #[test]
    fn dedication_before_first_heading() {
        let src = "Title: T\n\nIn memory of *A.*\n\nund B.\n\nC.\n\nD.\n\nE.\n\n# Eins\n\nX.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let shape: Vec<_> = book
            .sections
            .iter()
            .map(|s| (s.kind, s.blocks.len()))
            .collect();
        // Up to four paragraphs; markup inside one is kept.
        assert_eq!(
            shape,
            [
                (SectionKind::Dedication, 4),
                (SectionKind::Other, 1),
                (SectionKind::Chapter, 1)
            ]
        );
        assert_eq!(
            book.sections[0].blocks[0],
            Block::paragraph(vec![
                Inline::text("In memory of "),
                Inline::Emphasis {
                    content: vec![Inline::text("A.")]
                },
            ])
        );
        let got: Vec<_> = warnings.iter().map(|w| (w.kind, w.location)).collect();
        assert_eq!(got, [(WarningKind::TextBeforeFirstChapter, 11)]);

        // Not a dedication: long, or after other text, or not at the start.
        let long = format!("For {}.", "very ".repeat(60));
        for src in [
            format!("Title: T\n\n{long}\n\n# Eins\n"),
            "Title: T\n\nVorab.\n\nFür A.\n\n# Eins\n".to_string(),
            "Title: T\n\n# Eins\n\nFür A.\n".to_string(),
        ] {
            let book = import(&src).unwrap().book;
            assert!(
                book.sections
                    .iter()
                    .all(|s| s.kind != SectionKind::Dedication),
                "{src}"
            );
        }
    }

    #[test]
    fn explicit_chapter_numbers_are_stripped() {
        let src = "Title: T\n\n# Kapitel 3\n\nA.\n\n# Chapter 4: The Storm\n\nB.\n\n# Zweites Kapitel\n\nC.\n\n# Arrival\n\nD.\n";
        let book = import(src).unwrap().book;
        let got: Vec<_> = book
            .sections
            .iter()
            .map(|s| (s.title.as_deref(), s.numbering.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (None, Numbering::Explicit(3)),
                (Some("The Storm"), Numbering::Explicit(4)),
                (None, Numbering::Explicit(2)),
                (Some("Arrival"), Numbering::Auto),
            ]
        );
    }

    #[test]
    fn unnumbered_chapters() {
        let src = "Title: T\n\n# Arrival\n\nA.\n\n# Interlude {-}\n\nB.\n\n# Kapitel 3 {.unnumbered}\n\nC.\n\n# Departure\n\nD.\n";
        let book = import(src).unwrap().book;
        let got: Vec<_> = book
            .sections
            .iter()
            .map(|s| (s.title.as_deref(), s.numbering.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("Arrival"), Numbering::Auto),
                (Some("Interlude"), Numbering::None),
                (None, Numbering::None),
                (Some("Departure"), Numbering::Auto),
            ]
        );
    }

    #[test]
    fn inline_emphasis() {
        let v = parse_inlines(
            "a *b* and _c_ and **d** but snake_case_word and 2 * 3",
            &mut Notes::default(),
        );
        assert_eq!(
            v,
            vec![
                Inline::text("a "),
                Inline::Emphasis {
                    content: vec![Inline::text("b")]
                },
                Inline::text(" and "),
                Inline::Emphasis {
                    content: vec![Inline::text("c")]
                },
                Inline::text(" and "),
                Inline::Strong {
                    content: vec![Inline::text("d")]
                },
                Inline::text(" but snake_case_word and 2 * 3"),
            ]
        );
        // Strong inside emphasis, also at its end (`***`).
        let strong = |t: &str| Inline::Strong {
            content: vec![Inline::text(t)],
        };
        assert_eq!(
            parse_inlines("*a **b** c* *d **e***", &mut Notes::default()),
            vec![
                Inline::Emphasis {
                    content: vec![Inline::text("a "), strong("b"), Inline::text(" c")]
                },
                Inline::text(" "),
                Inline::Emphasis {
                    content: vec![Inline::text("d "), strong("e")]
                },
            ]
        );
    }

    #[test]
    fn small_caps_spans() {
        let v = parse_inlines(
            "Ein [Schild]{.smallcaps} und [*Mehr* [x]]{ #id .SmallCaps }, [unterstrichen]{.underline} [leer]{}[]{.smallcaps} [sic] ![a](b){.c} [a]{.smallcaps",
            &mut Notes::default(),
        );
        assert_eq!(
            v,
            vec![
                Inline::text("Ein "),
                Inline::SmallCaps {
                    content: vec![Inline::text("Schild")]
                },
                Inline::text(" und "),
                Inline::SmallCaps {
                    content: vec![
                        Inline::Emphasis {
                            content: vec![Inline::text("Mehr")]
                        },
                        Inline::text(" [x]"),
                    ]
                },
                // Other spans keep their text; brackets without `{…}` and
                // images are not spans.
                Inline::text(", unterstrichen leer [sic] ![a](b){.c} [a]{.smallcaps"),
            ]
        );
    }

    #[test]
    fn superscript_and_subscript() {
        let v = parse_inlines(
            "H~2~O, 1^st^ und 2^*nd*^; ~50 km, a ~ b, ~~weg~~, x^^, [^n] ^ende",
            &mut Notes::default(),
        );
        let t = Inline::text;
        assert_eq!(
            v,
            vec![
                t("H"),
                Inline::Subscript {
                    content: vec![t("2")]
                },
                t("O, 1"),
                Inline::Superscript {
                    content: vec![t("st")]
                },
                t(" und 2"),
                Inline::Superscript {
                    content: vec![Inline::Emphasis {
                        content: vec![t("nd")]
                    }]
                },
                t("; ~50 km, a ~ b, ~~weg~~, x^^, [^n] ^ende"),
            ]
        );
    }

    #[test]
    fn links() {
        let v = parse_inlines(
            "Siehe [die *Seite*](https://example.com/Tau_(Knoten) \"Titel\"), <https://example.com>, <mailto:a@example.com>, [](<https://x.example/a b>), [kein](), [Text] (nicht), <kein link>, ![bild](b.png)",
            &mut Notes::default(),
        );
        let t = Inline::text;
        let link = |url: &str, content| Inline::Link {
            url: url.into(),
            content,
        };
        assert_eq!(
            v,
            vec![
                t("Siehe "),
                link(
                    "https://example.com/Tau_(Knoten)",
                    vec![
                        t("die "),
                        Inline::Emphasis {
                            content: vec![t("Seite")]
                        }
                    ]
                ),
                t(", "),
                link("https://example.com", vec![t("https://example.com")]),
                t(", "),
                link("mailto:a@example.com", vec![t("a@example.com")]),
                t(", "),
                // Empty text shows the address.
                link("https://x.example/a b", vec![t("https://x.example/a b")]),
                t(", [kein](), [Text] (nicht), <kein link>, ![bild](b.png)"),
            ]
        );
    }

    #[test]
    fn sub_headings() {
        let src = "Title: T\n\n# Kapitel 1\n\nA.\n\n## Am *Hafen*\n\nB.\n\n### Tiefer ###\n\n## C#\n\n##\n\nC.\n\n####### sieben\n\n##kein\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(book.sections.len(), 1);
        assert_eq!(
            book.sections[0].blocks,
            vec![
                Block::paragraph(vec![Inline::text("A.")]),
                Block::heading(
                    1,
                    vec![
                        Inline::text("Am "),
                        Inline::Emphasis {
                            content: vec![Inline::text("Hafen")]
                        }
                    ]
                ),
                Block::paragraph(vec![Inline::text("B.")]),
                Block::heading(2, vec![Inline::text("Tiefer")]),
                Block::heading(1, vec![Inline::text("C#")]),
                // A bare `##` is a scene break; seven hashes or no space
                // after them are text.
                Block::scene_break(),
                Block::paragraph(vec![Inline::text("C.")]),
                Block::paragraph(vec![Inline::text("####### sieben")]),
                Block::paragraph(vec![Inline::text("##kein")]),
            ]
        );
    }

    #[test]
    fn sub_heading_before_first_chapter() {
        let Imported { book, warnings, .. } = import("## Vorab\n\n# Eins\n\nA.\n").unwrap();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.sections[0].kind, SectionKind::Other);
        assert_eq!(
            book.sections[0].blocks,
            vec![Block::heading(1, vec![Inline::text("Vorab")])]
        );
        let kinds: Vec<_> = warnings.iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![WarningKind::TextBeforeFirstChapter, WarningKind::NoTitle]
        );
        assert_eq!(warnings[0].location, 1);
    }

    #[test]
    fn block_quotes() {
        let src = "Title: T\n\n# Eins\n\nVorher.\n> Ich war in großer\n>Verlegenheit:\neine dringende Reise.\n>\n> > Zweiter *Absatz*.\n\n> Gut genug.\n***\n>\n>\n\nDanach.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let para = |t: &str| Block::paragraph(vec![Inline::text(t)]);
        assert_eq!(
            book.sections[0].blocks,
            vec![
                // A `>` line ends the paragraph before it.
                para("Vorher."),
                Block::block_quote(vec![
                    para("Ich war in großer Verlegenheit: eine dringende Reise."),
                    Block::paragraph(vec![
                        Inline::text("Zweiter "),
                        Inline::Emphasis {
                            content: vec![Inline::text("Absatz")]
                        },
                        Inline::text("."),
                    ]),
                ]),
                // A blank line starts a new quote; a scene break is not
                // lazily continued; bare `>` lines alone are no quote.
                Block::block_quote(vec![para("Gut genug.")]),
                Block::scene_break(),
                para("Danach."),
            ]
        );
    }

    #[test]
    fn lists() {
        let src = "Title: T\n\n# Eins\n\nWärme und Geruch\n- wie von Pferden kam hervor.\n\n- Erstes *Ding*,\nweiter.\n* Zweites[^n]\n\n- Drittes\n\n  Zweiter Absatz.\n  - innen\n  + auch innen\n\t- tiefer\n- Viertes\n1. Eins\n2) Zwei\n> Zitat\n\n3. Oktober ein Schwerkranker wartete auf mich.\n\n1. Mai einen Wagen hatte ich.\n\nDanach.\n\n7. Juli\n8. Juli\n\n3. Oktober wartete er auf mich, und am\n24. Dezember schwieg er.\n\n[^n]: Note.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let got: Vec<_> = warnings.iter().map(|w| (w.kind, w.location)).collect();
        assert_eq!(got, [(WarningKind::ListFlattened, 29)]);
        let para = |t: &str| Block::paragraph(vec![Inline::text(t)]);
        assert_eq!(
            book.sections[0].blocks,
            vec![
                // A list never interrupts a paragraph: hard-wrapped text can
                // put a spaced dash at the start of a line.
                para("Wärme und Geruch - wie von Pferden kam hervor."),
                Block::list(
                    false,
                    vec![
                        vec![Block::paragraph(vec![
                            Inline::text("Erstes "),
                            Inline::Emphasis {
                                content: vec![Inline::text("Ding")]
                            },
                            Inline::text(", weiter."),
                        ])],
                        vec![Block::paragraph(vec![
                            Inline::text("Zweites"),
                            Inline::Footnote {
                                blocks: vec![para("Note.")]
                            },
                        ])],
                        vec![
                            para("Drittes"),
                            para("Zweiter Absatz."),
                            Block::list(
                                false,
                                vec![
                                    vec![para("innen")],
                                    vec![
                                        para("auch innen"),
                                        Block::list(false, vec![vec![para("tiefer")]]),
                                    ],
                                ]
                            ),
                        ],
                        vec![para("Viertes")],
                    ]
                ),
                // Ordered after unordered is a new list.
                Block::list(true, vec![vec![para("Eins")], vec![para("Zwei")]]),
                Block::block_quote(vec![para("Zitat")]),
                // Ordered lists start at 1 and have two items.
                para("3. Oktober ein Schwerkranker wartete auf mich."),
                para("1. Mai einen Wagen hatte ich."),
                para("Danach."),
                // Counting on from 7: numbered paragraphs, with a warning.
                para("7. Juli"),
                para("8. Juli"),
                // A wrapped line starting with a date that is not the next
                // number continues the paragraph.
                para("3. Oktober wartete er auf mich, und am 24. Dezember schwieg er."),
            ]
        );
    }

    #[test]
    fn page_breaks() {
        let src = "Title: T\n\n\\newpage\n\n# Eins\n\n\\pagebreak\nA.\n\\newpage\n\\newpage\n> Zitat\n\\newpage\nB. \\newpage\n\n***\n\\pagebreak\n\n# Zwei\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let para = |t: &str| Block::paragraph(vec![Inline::text(t)]);
        // Before the first heading, at the start and the end of a section
        // a break is dropped; repeated ones collapse; a quote is not
        // lazily continued; inside a line it is text.
        assert_eq!(
            book.sections[0].blocks,
            vec![
                para("A."),
                Block::page_break(),
                Block::block_quote(vec![para("Zitat")]),
                Block::page_break(),
                para("B. \\newpage"),
            ]
        );
        assert!(book.sections[1].blocks.is_empty());
    }

    #[test]
    fn quote_stops_before_heading() {
        let book = import("Title: T\n\n# Eins\n\n> Zitat\n## Unter\n# Zwei\n\nB.\n")
            .unwrap()
            .book;
        assert_eq!(
            book.sections[0].blocks,
            vec![
                Block::block_quote(vec![Block::paragraph(vec![Inline::text("Zitat")])]),
                Block::heading(1, vec![Inline::text("Unter")]),
            ]
        );
        assert_eq!(book.sections.len(), 2);
    }

    #[test]
    fn footnotes() {
        let src = "Title: T\n\n# Eins\n\nIch bin kein Weltverbesserer [^r], *und lasse ihn liegen[^w]*.\n\n> Noch standen treu die Pferde[^r].\n\n[^r]: Dort sitzt er\nTage und *Jahre*.\n\n    Nun lebt er\n    nicht mehr lange.\n\n[^w]: Mit [^r] darin.\nDanach.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let rain = Inline::Footnote {
            blocks: vec![
                Block::paragraph(vec![
                    Inline::text("Dort sitzt er Tage und "),
                    Inline::Emphasis {
                        content: vec![Inline::text("Jahre")],
                    },
                    Inline::text("."),
                ]),
                Block::paragraph(vec![Inline::text("Nun lebt er nicht mehr lange.")]),
            ],
        };
        assert_eq!(
            book.sections[0].blocks,
            vec![
                // The space before a mark is dropped; a mark inside a note
                // stays text; a plain line continues the definition.
                Block::paragraph(vec![
                    Inline::text("Ich bin kein Weltverbesserer"),
                    rain.clone(),
                    Inline::text(", "),
                    Inline::Emphasis {
                        content: vec![
                            Inline::text("und lasse ihn liegen"),
                            Inline::Footnote {
                                blocks: vec![Block::paragraph(vec![Inline::text(
                                    "Mit [^r] darin. Danach."
                                )])],
                            },
                        ],
                    },
                    Inline::text("."),
                ]),
                Block::block_quote(vec![Block::paragraph(vec![
                    Inline::text("Noch standen treu die Pferde"),
                    rain,
                    Inline::text("."),
                ])]),
            ]
        );
    }

    #[test]
    fn footnote_problems_warn() {
        let src = "Title: T\n\n# Eins\n\nA.\n\nB[^x] und [^ y].\n\n# Zwei\n\n[^a]: Ungenutzt.\n\n[^x]: Ja.\n\n[^x]: Doppelt.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        assert_eq!(
            got,
            vec![
                (WarningKind::FootnoteUnused, 11, None),
                (WarningKind::FootnoteUnused, 15, None),
            ]
        );
        // `[^ y]` is no mark (space in the label); definitions are gone
        // from the text, so "Zwei" is empty.
        assert_eq!(
            Inline::plain_text(match &book.sections[0].blocks[1] {
                Block::Paragraph { content, .. } => content,
                _ => panic!(),
            }),
            "B und [^ y]."
        );
        assert!(book.sections[1].blocks.is_empty());

        let Imported { warnings, .. } = import("Title: T\n\n# Eins\n\nA.\n\nB[^9].\n").unwrap();
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        assert_eq!(got, vec![(WarningKind::FootnoteMissing, 7, Some(0))]);
        assert!(
            warnings[0].message.contains("[^9]"),
            "{}",
            warnings[0].message
        );
    }

    #[test]
    fn verse() {
        let src = "Title: T\n\n# Eins\n\nVorher.\n:::verse\n\nDer *Wind* geht,\\\n  das Meer[^m] steht.\n\n\nEine trübe Stallaterne schwankte drin an einem Seil\n\n:::\n> Zitat\n::: Verse :::\nNun bin ich aber Arzt\n::::\n\n:::verse\n\n:::\n:::\n\n[^m]: Das Dienstmädchen stand neben mir.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let para = |t: &str| Block::paragraph(vec![Inline::text(t)]);
        assert_eq!(
            book.sections[0].blocks,
            vec![
                para("Vorher."),
                // Blank lines at the ends dropped, runs of them make one
                // stanza break; `\` and indentation dropped.
                Block::verse(vec![
                    vec![
                        Inline::text("Der "),
                        Inline::Emphasis {
                            content: vec![Inline::text("Wind")]
                        },
                        Inline::text(" geht,"),
                    ],
                    vec![
                        Inline::text("das Meer"),
                        Inline::Footnote {
                            blocks: vec![para("Das Dienstmädchen stand neben mir.")]
                        },
                        Inline::text(" steht."),
                    ],
                    vec![],
                    vec![Inline::text(
                        "Eine trübe Stallaterne schwankte drin an einem Seil"
                    )],
                ]),
                // A fence is not lazily continued into a quote.
                Block::block_quote(vec![para("Zitat")]),
                Block::verse(vec![vec![Inline::text("Nun bin ich aber Arzt")]]),
                // An empty fence adds nothing; a stray `:::` stays text.
                para(":::"),
            ]
        );
    }

    /// The indent levels of the first verse block of the first section, and
    /// its lines as plain text.
    fn verse_levels_of(fence: &str) -> (Vec<u8>, Vec<String>) {
        let src = format!("Title: T\n\n# Eins\n\n:::verse\n{fence}:::\n");
        let Imported { book, warnings, .. } = import(&src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let b = &book.sections[0].blocks[0];
        let Block::Verse { lines, .. } = b else {
            panic!("not verse: {b:?}");
        };
        let levels = (0..lines.len()).map(|i| b.verse_indent(i)).collect();
        let text = lines.iter().map(|l| Inline::plain_text(l)).collect();
        (levels, text)
    }

    // Carroll's "How doth the little crocodile" (Gutenberg #11): the
    // stanza the verse tests use, in the source's own four spaces.
    const CROC: &str = "“How doth the little crocodile\n    Improve his shining tail,\nAnd pour the waters of the Nile\n    On every golden scale!\n";

    #[test]
    fn verse_indents_one_level_per_four_spaces() {
        let (levels, text) = verse_levels_of(CROC);
        assert_eq!(levels, [0, 1, 0, 1]);
        // The text is the line, trimmed.
        assert_eq!(text[1], "Improve his shining tail,");
        // A model with indents writes them; one without writes none.
        let src = format!("Title: T\n\n# Eins\n\n:::verse\n{CROC}:::\n");
        let b = &import(&src).unwrap().book.sections[0].blocks[0];
        assert_eq!(
            serde_json::to_value(b).unwrap()["indents"],
            serde_json::json!([0, 1, 0, 1])
        );
        let flat = "“How doth the little crocodile\nAnd pour the waters of the Nile\n";
        let src = format!("Title: T\n\n# Eins\n\n:::verse\n{flat}:::\n");
        let b = &import(&src).unwrap().book.sections[0].blocks[0];
        assert!(serde_json::to_value(b).unwrap().get("indents").is_none());
    }

    #[test]
    fn verse_indent_steps_tabs_and_stray_spaces() {
        // 1-3 stray spaces are no indent, 4-7 one level, 8 two; a tab is
        // four spaces; mixing counts columns.
        let lines = [
            "“How doth the little crocodile",
            " Improve his shining tail,",
            "  And pour the waters of the Nile",
            "   On every golden scale!",
            "    “How cheerfully he seems to grin,",
            "       How neatly spread his claws,",
            "        And welcome little fishes in",
            "\tWith gently smiling jaws!”",
            "\t\tHow doth the little crocodile",
            "  \tImprove his shining tail,",
        ];
        let (levels, _) = verse_levels_of(&(lines.join("\n") + "\n"));
        assert_eq!(levels, [0, 0, 0, 0, 1, 1, 2, 1, 2, 1]);
    }

    #[test]
    fn verse_indent_is_counted_from_the_least_indented_line() {
        // A verse typed with every line indented is flush, like a code
        // block; the levels are relative to the block's least-indented line
        // (here 2 columns), also across stanzas.
        let fence = "  “How doth the little crocodile\n      Improve his shining tail,\n  And pour the waters of the Nile\n      On every golden scale!\n\n      “How cheerfully he seems to grin,\n";
        let (levels, _) = verse_levels_of(fence);
        assert_eq!(levels, [0, 1, 0, 1, 0, 1]);
    }

    #[test]
    fn verse_indent_blank_lines_ends_and_the_deepest_level() {
        // Blank lines (and a line that is only `\`) are stanza gaps, none at
        // the ends, and flush; the levels follow the lines kept. Thirty-six
        // columns would be nine levels: the model stops at six.
        let deep = " ".repeat(36);
        let fence = format!(
            "\n    “How doth the little crocodile\\\nAnd pour the waters of the Nile\n \\\n\n\n{deep}On every golden scale!\n\n"
        );
        let (levels, text) = verse_levels_of(&fence);
        assert_eq!(text.len(), 4, "{text:?}");
        assert!(text[2].is_empty());
        assert_eq!(levels, [1, 0, 0, MAX_VERSE_INDENT]);
    }

    #[test]
    fn epigraphs() {
        let src = "Title: T\n\n:::epigraph\nVorn *steht* es.\n— Jemand, *Buch*\n:::\n\n# Teil 1\n\n::: Epigraph :::\nOhne Zuschreibung,\\\nin zwei Zeilen.\n\nArmer Junge, dir ist nicht zu helfen[^n].\n:::\n\n# Eins\n\n:::epigraph\n— Nur ein Strich\n:::\n\nA.\n\n:::epigraph\nSo ist es.\n-- Wer\n:::\n\n# Zwei\n\n:::epigraph\nOffen\n\n[^n]: Note.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let text = |t: &str| Inline::text(t);
        let para = |t: &str| Block::paragraph(vec![text(t)]);
        let kinds: Vec<_> = book.sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::Epigraph,
                SectionKind::Part,
                SectionKind::Chapter,
                SectionKind::Chapter
            ]
        );
        // Before the first heading: a section of its own; markup in the
        // attribution is dropped.
        assert_eq!(
            book.sections[0].epigraph,
            Some(Epigraph {
                text: vec![
                    text("Vorn "),
                    Inline::Emphasis {
                        content: vec![text("steht")]
                    },
                    text(" es."),
                ],
                attribution: Some("Jemand, Buch".into()),
            })
        );
        assert!(book.sections[0].blocks.is_empty());
        // A blank line is a line break; notes work.
        assert_eq!(
            book.sections[1].epigraph,
            Some(Epigraph {
                text: vec![
                    text("Ohne Zuschreibung,"),
                    Inline::LineBreak,
                    text("in zwei Zeilen."),
                    Inline::LineBreak,
                    text("Armer Junge, dir ist nicht zu helfen"),
                    Inline::Footnote {
                        blocks: vec![para("Note.")]
                    },
                    text("."),
                ],
                attribution: None,
            })
        );
        // A dash line alone is text; inside the chapter it is a quote.
        let eins = &book.sections[2];
        assert_eq!(
            eins.epigraph,
            Some(Epigraph {
                text: vec![text("— Nur ein Strich")],
                attribution: None,
            })
        );
        assert_eq!(
            eins.blocks,
            vec![
                para("A."),
                Block::block_quote(vec![para("So ist es."), para("-- Wer")]),
            ]
        );
        assert_eq!(
            book.sections[3].epigraph.as_ref().map(|e| &e.text),
            Some(&vec![text("Offen")])
        );
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        assert_eq!(got, [(WarningKind::VerseNotClosed, 32, Some(3))]);
        assert!(warnings[0].message.contains("`:::epigraph`"));
    }

    #[test]
    fn verse_not_closed() {
        let src = "Title: T\n\n# Eins\n\n:::verse\nZeile\n\n# Zwei\n\nA.\n\n:::verse\nEnde\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        assert_eq!(
            got,
            vec![
                (WarningKind::VerseNotClosed, 5, Some(0)),
                (WarningKind::VerseNotClosed, 12, Some(1)),
            ]
        );
        assert_eq!(book.sections.len(), 2);
        assert_eq!(
            book.sections[0].blocks,
            vec![Block::verse(vec![vec![Inline::text("Zeile")]])]
        );
        assert_eq!(
            book.sections[1].blocks[1],
            Block::verse(vec![vec![Inline::text("Ende")]])
        );
    }

    #[test]
    fn hard_line_breaks() {
        let src = "Title: T\n\n# Eins\n\nLiebe *Anna,\\\ndas Haus* ist \\\nverkauft.\\\n\n> Gruß \\\n> Lena\n\nB[^n].\n\n[^n]: Eins\\\nzwei.\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            book.sections[0].blocks,
            vec![
                // Emphasis spans a break; a `\` on the last line is dropped.
                Block::paragraph(vec![
                    Inline::text("Liebe "),
                    Inline::Emphasis {
                        content: vec![
                            Inline::text("Anna,"),
                            Inline::LineBreak,
                            Inline::text("das Haus"),
                        ]
                    },
                    Inline::text(" ist"),
                    Inline::LineBreak,
                    Inline::text("verkauft."),
                ]),
                Block::block_quote(vec![Block::paragraph(vec![
                    Inline::text("Gruß"),
                    Inline::LineBreak,
                    Inline::text("Lena"),
                ])]),
                Block::paragraph(vec![
                    Inline::text("B"),
                    Inline::Footnote {
                        blocks: vec![Block::paragraph(vec![
                            Inline::text("Eins"),
                            Inline::LineBreak,
                            Inline::text("zwei."),
                        ])]
                    },
                    Inline::text("."),
                ]),
            ]
        );
    }

    #[test]
    fn images() {
        let src = "Title: T\n\n# Eins\n\nVorher.\n![Die *Karte*](img/karte.png \"Der neue Advokat[^q], 1921\")\n\n![](<mein bild.jpeg>){width=50%}\n\n![Nochmal](img/karte.png)\n\n![a](b.png) mit Text\n\n[^q]: Archiv.\n";
        let mut asked = Vec::new();
        let Imported {
            book,
            assets,
            warnings,
        } = super::import(src, |p| {
            asked.push(p.to_string());
            match p {
                "img/karte.png" => Some(PNG.to_vec()),
                "mein bild.jpeg" => Some(vec![0xFF, 0xD8, 0xFF, 0xE0]),
                _ => None,
            }
        })
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        // Each path is asked for once; the text line is no image.
        assert_eq!(asked, ["img/karte.png", "mein bild.jpeg"]);
        let ids: Vec<_> = assets
            .iter()
            .map(|a| (a.id.as_str(), a.media_type.as_str()))
            .collect();
        assert_eq!(
            ids,
            [("karte.png", "image/png"), ("mein_bild.jpg", "image/jpeg")]
        );
        assert_eq!(
            book.sections[0].blocks,
            vec![
                Block::paragraph(vec![Inline::text("Vorher.")]),
                Block::image(
                    "karte.png",
                    "Die Karte",
                    Some(vec![
                        Inline::text("Der neue Advokat"),
                        Inline::Footnote {
                            blocks: vec![Block::paragraph(vec![Inline::text("Archiv.")])]
                        },
                        Inline::text(", 1921"),
                    ])
                ),
                Block::image("mein_bild.jpg", "", None).with_width(50),
                Block::image("karte.png", "Nochmal", None),
                Block::paragraph(vec![Inline::text("![a](b.png) mit Text")]),
            ]
        );
    }

    #[test]
    fn image_problems_warn() {
        let src = "Title: T\n\n# Eins\n\n![a](fehlt.png)\n\n![b](fehlt.png)\n\n![c](https://example.com/c.png)\n\n![d](alt.tiff)\n\n> Zitat\n![e](gross.png)\n";
        let Imported {
            book,
            assets,
            warnings,
        } = super::import(src, |p| match p {
            "alt.tiff" => Some(b"II*\0rest".to_vec()),
            "gross.png" => Some(vec![0; MAX_IMAGE_BYTES as usize + 1]),
            _ => None,
        })
        .unwrap();
        let got: Vec<_> = warnings
            .iter()
            .map(|w| (w.kind, w.location, w.section))
            .collect();
        // A missing path warns once; a URL is not asked for.
        assert_eq!(
            got,
            vec![
                (WarningKind::ImageMissing, 5, Some(0)),
                (WarningKind::ImageMissing, 9, Some(0)),
                (WarningKind::ImageUnsupported, 11, Some(0)),
                (WarningKind::ImageMissing, 14, Some(0)),
            ]
        );
        assert!(
            warnings[2].message.contains("TIFF"),
            "{}",
            warnings[2].message
        );
        assert!(assets.is_empty());
        // Images that cannot be used are left out; an image line is not
        // lazily continued into a quote.
        assert_eq!(
            book.sections[0].blocks,
            vec![Block::block_quote(vec![Block::paragraph(vec![
                Inline::text("Zitat")
            ])])]
        );
    }

    #[test]
    fn image_line_parts() {
        fn parts(l: &str) -> Option<(&str, &str, Option<&str>)> {
            image_line(l).map(|i| (i.alt, i.path, i.caption))
        }
        assert_eq!(parts("![a](b.png)"), Some(("a", "b.png", None)));
        assert_eq!(
            parts("![a]( my file.png  \"Cap \"x\"\" )"),
            Some(("a", "my file.png", Some("Cap \"x\"")))
        );
        assert_eq!(parts("![a](b.png\"x\")"), Some(("a", "b.png\"x\"", None)));
        assert_eq!(
            parts("![a](<b c.png>){#fig .wide}"),
            Some(("a", "b c.png", None))
        );
        assert_eq!(parts("![a]()"), None);
        assert_eq!(parts("![a](b.png) text"), None);
        assert_eq!(parts("[a](b.png)"), None);
    }

    #[test]
    fn comments_keep_line_numbers() {
        let src = "Title: T\n\n<!--\nEine Notiz\n-->\n# Eins\n\nA <!-- x -->B[^9].\n";
        let Imported { book, warnings, .. } = import(src).unwrap();
        assert_eq!(book.sections.len(), 1);
        let got: Vec<_> = warnings.iter().map(|w| (w.kind, w.location)).collect();
        assert_eq!(got, [(WarningKind::FootnoteMissing, 8)]);
        assert_eq!(
            book.sections[0].blocks,
            vec![Block::paragraph(vec![Inline::text("A B[^9].")])]
        );
    }

    /// The tables of the first section, with the warnings of the import.
    fn pipe_tables(src: &str) -> (Vec<Table>, Vec<ImportWarning>) {
        let Imported { book, warnings, .. } = import(&format!("Title: T\n\n# C\n\n{src}")).unwrap();
        let tables = book.sections[0]
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Table(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        (tables, warnings)
    }

    /// A table's rows as the plain text of each cell.
    fn texts(t: &Table) -> Vec<Vec<String>> {
        t.rows
            .iter()
            .map(|r| r.iter().map(|c| Inline::plain_text(&c.content)).collect())
            .collect()
    }

    fn aligns(t: &Table) -> Vec<ColumnAlign> {
        t.columns.iter().map(|c| c.align).collect()
    }

    #[test]
    fn a_pipe_table_is_a_table_with_alignment_and_a_header_row() {
        let (tables, warnings) = pipe_tables(
            "| A | B | C | D |\n|:--|--:|:-:|---|\n| 1 | 2 | 3 | 4 |\n| 5 | 6 | 7 | 8 |\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let [t] = tables.as_slice() else { panic!() };
        use ColumnAlign::{Center, Left, Right};
        assert_eq!(aligns(t), [Left, Right, Center, Left]);
        assert_eq!(t.header_rows, 1);
        assert_eq!(texts(t)[0], ["A", "B", "C", "D"]);
        assert_eq!(texts(t)[2], ["5", "6", "7", "8"]);
        assert_eq!(t.caption, None);
        assert!(
            t.rows
                .iter()
                .flatten()
                .all(|c| c.colspan == 1 && c.rowspan == 1 && c.align.is_none())
        );
    }

    #[test]
    fn a_table_without_outer_pipes_and_with_a_single_dash_delimiter() {
        let (tables, warnings) = pipe_tables("A | B\n-: | :-\n1 | 2\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        let [t] = tables.as_slice() else { panic!() };
        assert_eq!(aligns(t), [ColumnAlign::Right, ColumnAlign::Left]);
        assert_eq!(texts(t), [["A", "B"], ["1", "2"]]);
    }

    #[test]
    fn an_empty_header_row_is_no_header() {
        let (tables, warnings) = pipe_tables("| | |\n|:-|-:|\n| a | b |\n| c | d |\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        let [t] = tables.as_slice() else { panic!() };
        assert_eq!(t.header_rows, 0);
        assert_eq!(texts(t), [["a", "b"], ["c", "d"]]);
        assert_eq!(aligns(t), [ColumnAlign::Left, ColumnAlign::Right]);
        // Part of the header empty: still the header.
        let (tables, _) = pipe_tables("| a | |\n|-|-|\n| | b |\n");
        assert_eq!(tables[0].header_rows, 1);
        assert_eq!(texts(&tables[0]), [["a", ""], ["", "b"]]);
    }

    #[test]
    fn a_header_alone_is_a_table() {
        let (tables, warnings) = pipe_tables("| a | b |\n|-|-|\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(tables[0].header_rows, 1);
        assert_eq!(texts(&tables[0]), [["a", "b"]]);
    }

    #[test]
    fn short_rows_are_padded_and_long_rows_are_cut_with_a_warning() {
        let (tables, warnings) =
            pipe_tables("| a | b | c |\n|-|-|-|\n| 1 |\n| 2 | 3 |\n| 4 | 5 | 6 | 7 |\n");
        let [t] = tables.as_slice() else { panic!() };
        assert_eq!(
            texts(t),
            [
                ["a", "b", "c"],
                ["1", "", ""],
                ["2", "3", ""],
                ["4", "5", "6"]
            ]
        );
        let got: Vec<_> = warnings.iter().map(|w| (w.kind, w.location)).collect();
        assert_eq!(got, [(WarningKind::TableSimplified, 5)]);
        // Empty cells beyond the header lose nothing: no warning.
        let (_, warnings) = pipe_tables("| a |\n|-|\n| 1 | |\n");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_table_with_no_text_is_dropped_with_a_warning() {
        let (tables, warnings) = pipe_tables("Before.\n\n| | |\n|-|-|\n| | |\n\nAfter.\n");
        assert!(tables.is_empty());
        let got: Vec<_> = warnings.iter().map(|w| (w.kind, w.location)).collect();
        assert_eq!(got, [(WarningKind::TableSimplified, 7)]);
    }

    #[test]
    fn cells_hold_inlines_an_escaped_pipe_and_a_footnote() {
        let (tables, warnings) = pipe_tables(
            "| a \\| b | *c* |\n|-|-|\n| **d**[^1] | [e](https://x.org) |\n\n[^1]: Note.\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let t = &tables[0];
        assert_eq!(texts(t)[0], ["a | b", "c"]);
        assert!(matches!(
            t.rows[0][1].content.as_slice(),
            [Inline::Emphasis { .. }]
        ));
        assert!(matches!(
            t.rows[1][0].content.as_slice(),
            [Inline::Strong { .. }, Inline::Footnote { .. }]
        ));
        assert!(matches!(
            t.rows[1][1].content.as_slice(),
            [Inline::Link { .. }]
        ));
    }

    #[test]
    fn a_footnote_mark_in_a_dropped_cell_is_an_unused_note() {
        let (tables, warnings) = pipe_tables("| a |\n|-|\n| 1 | x[^1] |\n\n[^1]: Note.\n");
        assert_eq!(texts(&tables[0]), [["a"], ["1"]]);
        let kinds: Vec<_> = warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&WarningKind::TableSimplified), "{kinds:?}");
        assert!(kinds.contains(&WarningKind::FootnoteUnused), "{kinds:?}");
    }

    #[test]
    fn a_one_column_table_is_a_table_and_prose_is_not() {
        let (tables, warnings) = pipe_tables("| a |\n|---|\n| b |\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(texts(&tables[0]), [["a"], ["b"]]);
        let (tables, _) = pipe_tables("A line with a | pipe.\nAnother | one.\n");
        assert!(tables.is_empty());
        let (tables, _) = pipe_tables("| no delimiter |\n| row here |\n");
        assert!(tables.is_empty());
    }

    /// The caption of a table as plain text.
    fn caption(t: &Table) -> Option<String> {
        t.caption.as_deref().map(Inline::plain_text)
    }

    #[test]
    fn a_table_caption_above_or_below() {
        let rows = "| a |\n|-|\n| 1 |\n";
        // Above, directly or after a blank line; below, the same.
        for src in [
            format!("Table: Cap\n{rows}"),
            format!("Table: Cap\n\n{rows}"),
            format!("{rows}Table: Cap\n"),
            format!("{rows}\nTable: Cap\n"),
            format!("{rows}\nTable: Cap\n\nAfter.\n"),
        ] {
            let (tables, warnings) = pipe_tables(&src);
            assert!(warnings.is_empty(), "{src:?}: {warnings:?}");
            assert_eq!(caption(&tables[0]).as_deref(), Some("Cap"), "{src:?}");
            assert_eq!(texts(&tables[0]), [["a"], ["1"]]);
        }
    }

    #[test]
    fn a_caption_line_that_is_not_a_caption_stays_a_paragraph() {
        let rows = "| a |\n|-|\n| 1 |\n";
        for src in [
            // Part of a paragraph, not alone in it.
            format!("Some prose\nTable: Cap\n\n{rows}"),
            format!("Some prose\nTable: Cap\n{rows}"),
            format!("{rows}Table: Cap\nmore of the paragraph\n"),
            // No text, or no space after the colon.
            format!("Table:\n\n{rows}"),
            format!("Table:Cap\n\n{rows}"),
            // Two blank lines away.
            format!("Table: Cap\n\n\n{rows}"),
            format!("{rows}\n\nTable: Cap\n"),
        ] {
            let (tables, _) = pipe_tables(&src);
            assert_eq!(caption(&tables[0]), None, "{src:?}");
        }
        let (tables, _) = pipe_tables("Table: Cap\n\nJust prose.\n");
        assert!(tables.is_empty());
    }

    #[test]
    fn a_caption_between_two_tables_belongs_to_the_one_below() {
        let src = "| a |\n|-|\n| 1 |\n\nTable: Middle\n\n| b |\n|-|\n| 2 |\n";
        let (tables, _) = pipe_tables(src);
        assert_eq!(caption(&tables[0]), None);
        assert_eq!(caption(&tables[1]).as_deref(), Some("Middle"));
        // A caption above and one below: the table keeps the first, the
        // second is a paragraph.
        let (tables, _) = pipe_tables("Table: One\n| a |\n|-|\n| 1 |\nTable: Two\n");
        assert_eq!(caption(&tables[0]).as_deref(), Some("One"));
    }

    #[test]
    fn a_caption_is_inlines_and_may_hold_a_footnote() {
        let (tables, warnings) =
            pipe_tables("Table: *Cap*[^1]\n\n| a |\n|-|\n| 1 |\n\n[^1]: Note.\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = tables[0].caption.as_deref().unwrap();
        assert!(matches!(
            c,
            [Inline::Emphasis { .. }, Inline::Footnote { .. }]
        ));
    }

    #[test]
    fn the_caption_of_a_table_with_no_text_stays_a_paragraph() {
        let Imported { book, warnings, .. } =
            import("Title: T\n\n# C\n\nTable: Cap\n| |\n|-|\n| |\n").unwrap();
        assert_eq!(
            book.sections[0].blocks,
            vec![Block::paragraph(vec![Inline::text("Cap")])]
        );
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn a_caption_below_ends_at_a_line_that_starts_a_block() {
        let rows = "| a |\n|-|\n| 1 |\n";
        for next in ["## Heading", "> A quote", "* * *", "\\newpage"] {
            let (tables, _) = pipe_tables(&format!("{rows}Table: Cap\n{next}\n"));
            assert_eq!(caption(&tables[0]).as_deref(), Some("Cap"), "{next:?}");
        }
        // A line of prose continues the paragraph, so it is no caption.
        let (tables, _) = pipe_tables(&format!("{rows}Table: Cap\nprose\n"));
        assert_eq!(caption(&tables[0]), None);
    }

    #[test]
    fn a_table_before_the_first_heading_warns_in_its_section() {
        let Imported { warnings, .. } = import("Title: T\n\n| a |\n|-|\n| 1 | x |\n").unwrap();
        let w = warnings
            .iter()
            .find(|w| w.kind == WarningKind::TableSimplified)
            .expect("TableSimplified");
        assert!(w.section.is_some(), "{warnings:?}");
    }

    #[test]
    fn a_dropped_table_puts_its_caption_at_the_caption_line() {
        let Imported { warnings, .. } = import("Title: T\n\nTable: Cap\n| |\n|-|\n| |\n").unwrap();
        let w = warnings
            .iter()
            .find(|w| w.kind == WarningKind::TextBeforeFirstChapter)
            .expect("TextBeforeFirstChapter");
        assert_eq!(w.location, 3, "{warnings:?}");
    }

    #[test]
    fn empty_is_error() {
        assert!(import("  \n").is_err());
        assert!(import("<!-- nur eine Notiz -->\n").is_err());
    }
}
