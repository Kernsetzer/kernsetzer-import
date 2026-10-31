//! The grid of a table of the body: which cell starts where,
//! and how far it reaches. `story` reads the rows of a `w:tbl` as Word wrote
//! them, one entry per `w:tc` (`RawRow`); `resolve` turns them into the
//! cells the model lists, row by row, only those that start in the row.
//!
//! Word writes a horizontal merge as one `w:tc` with `w:gridSpan` and a
//! vertical one as a column of `w:tc`: the first has `w:vMerge
//! w:val="restart"`, each one below has `w:vMerge` (continue) and holds
//! nothing. The legacy `w:hMerge` (a restart cell, then continue cells) says
//! the same as a `gridSpan`. `w:gridBefore` and `w:gridAfter` on a row are
//! grid columns with no cell at its start and end.
//!
//! A span is only ever made from Word's own attributes. A continue cell
//! with nothing to continue (no restart above it or beside it, or one at
//! another column or width) is an ordinary cell, and the table is said to
//! be simplified. A continue cell that holds text is not lost either: the
//! text goes to the cell it continues.

use super::story::{Piece, Seg, plain_seg};
use super::styles::Align;

/// The widest a span, `gridBefore` or `gridAfter` read from a file can be:
/// the model's limit for a table whose width is taken from its rows.
pub(super) const MAX_SPAN: usize = 64;

/// What `w:vMerge` or `w:hMerge` says about a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Merge {
    #[default]
    None,
    /// `w:val="restart"`: the cell starts a merged range.
    Restart,
    /// No `w:val` (or `continue`): the cell is covered by the one before it
    /// (beside it, or above it).
    Continue,
}

/// One `w:tc`, as read.
#[derive(Debug, Clone, Default)]
pub(super) struct RawCell {
    pub segs: Vec<Seg>,
    pub align: Align,
    /// `w:gridSpan` (1 when absent or 0).
    pub span: usize,
    pub v_merge: Merge,
    pub h_merge: Merge,
}

/// One `w:tr`, as read.
#[derive(Debug, Clone, Default)]
pub(super) struct RawRow {
    /// `w:gridBefore`, `w:gridAfter`: grid columns with no cell.
    pub before: usize,
    pub after: usize,
    pub cells: Vec<RawCell>,
    pub header: bool,
}

/// A cell the model lists: where it starts and how far it reaches.
#[derive(Debug, Clone, Default)]
pub(super) struct GridCell {
    pub segs: Vec<Seg>,
    pub align: Align,
    /// The grid column the cell starts in.
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
}

/// The table, resolved.
#[derive(Debug, Default)]
pub(super) struct Grid {
    /// Per row, the cells that start in it, in column order.
    pub rows: Vec<Vec<GridCell>>,
    /// The width of the grid.
    pub columns: usize,
    /// The most `w:tc` cells in a row, a horizontal merge counted once: what
    /// a reader sees as columns when every cell is a merged one.
    pub cells_wide: usize,
    /// The leading `w:tblHeader` rows, grown to the end of a span that
    /// starts in them.
    pub header_rows: usize,
    /// A continue cell had nothing to continue and stayed an ordinary cell.
    pub orphan: bool,
    /// A continue cell held text, which went to the cell it continues.
    pub covered_text: bool,
}

fn has_content(segs: &[Seg]) -> bool {
    segs.iter().any(|s| match &s.piece {
        Piece::Text(t) => !t.trim().is_empty(),
        Piece::LineBreak => false,
        Piece::Note(_) | Piece::Image { .. } => true,
    })
}

/// Add the content of a covered cell to the cell that covers it.
fn append(into: &mut Vec<Seg>, from: Vec<Seg>) {
    if has_content(into) {
        into.push(plain_seg(Piece::LineBreak));
    }
    into.extend(from);
}

/// Place the cells of `raw` on the grid.
pub(super) fn resolve(raw: Vec<RawRow>) -> Grid {
    let mut grid = Grid::default();
    // Step 1: per row, fold the cells of a horizontal merge into one, and
    // give every cell its column.
    struct Folded {
        cell: RawCell,
        col: usize,
        // The cell is the start or a continuation of an `hMerge` range.
        h_chain: bool,
    }
    // Per row: its cells and the grid column after the last of them.
    let mut folded: Vec<(Vec<Folded>, usize)> = Vec::new();
    let mut extent = 0usize;
    let mut cells_in_a_row = 0usize;
    for row in &raw {
        let mut col = row.before.min(MAX_SPAN);
        let mut cells: Vec<Folded> = Vec::new();
        for cell in &row.cells {
            let mut cell = cell.clone();
            cell.span = cell.span.clamp(1, MAX_SPAN);
            let span = cell.span;
            let joins =
                cell.h_merge == Merge::Continue && cells.last().is_some_and(|last| last.h_chain);
            if joins && let Some(last) = cells.last_mut() {
                last.cell.span = (last.cell.span + span).min(MAX_SPAN);
                if has_content(&cell.segs) {
                    append(&mut last.cell.segs, cell.segs);
                    grid.covered_text = true;
                }
            } else {
                grid.orphan |= cell.h_merge == Merge::Continue;
                cells.push(Folded {
                    h_chain: cell.h_merge != Merge::None,
                    cell,
                    col,
                });
            }
            col += span;
        }
        extent = extent.max(col + row.after.min(MAX_SPAN));
        cells_in_a_row = cells_in_a_row.max(cells.len());
        folded.push((cells, col));
    }
    grid.columns = extent.min(MAX_SPAN.max(cells_in_a_row)).max(1);
    grid.cells_wide = cells_in_a_row;

    // Step 2: vertical merges. A restart cell reaches down through the rows
    // right below it that hold a continue cell at the same column and the
    // same width. `open` is the cell (row, index in the row's list) that
    // covers a column range in the row above.
    let mut rows: Vec<Vec<GridCell>> = Vec::new();
    let mut open: Vec<((usize, usize), (usize, usize))> = Vec::new();
    for (r, (cells, end)) in folded.into_iter().enumerate() {
        let before = raw[r].before.min(MAX_SPAN);
        let mut out: Vec<GridCell> = Vec::new();
        let mut now: Vec<((usize, usize), (usize, usize))> = Vec::new();
        if before > 0 {
            out.push(GridCell {
                col: 0,
                colspan: before,
                rowspan: 1,
                ..GridCell::default()
            });
        }
        for f in cells {
            let key = (f.col, f.cell.span);
            let above = open.iter().find(|(k, _)| *k == key).map(|(_, at)| *at);
            if f.cell.v_merge == Merge::Continue {
                if let Some((ar, ai)) = above {
                    // Covered: not listed, and the cell above reaches down.
                    let anchor = &mut rows[ar][ai];
                    anchor.rowspan += 1;
                    if has_content(&f.cell.segs) {
                        append(&mut anchor.segs, f.cell.segs);
                        grid.covered_text = true;
                    }
                    now.push((key, (ar, ai)));
                    continue;
                }
                grid.orphan = true;
            }
            if f.cell.v_merge == Merge::Restart {
                now.push((key, (r, out.len())));
            }
            out.push(GridCell {
                segs: f.cell.segs,
                align: f.cell.align,
                col: f.col,
                colspan: f.cell.span,
                rowspan: 1,
            });
        }
        let after = raw[r].after.min(MAX_SPAN);
        if after > 0 {
            out.push(GridCell {
                col: end,
                colspan: after,
                rowspan: 1,
                ..GridCell::default()
            });
        }
        open = now;
        rows.push(out);
    }

    // Step 3: header rows are the leading marked ones, and a span that
    // starts in them reaches the end of its rows (the author drew it as
    // part of the header; the model would cut it at the boundary).
    let marked = raw.iter().take_while(|r| r.header).count();
    let mut header = marked;
    loop {
        let grown = rows
            .iter()
            .take(header)
            .enumerate()
            .flat_map(|(r, cells)| cells.iter().map(move |c| r + c.rowspan))
            .max()
            .unwrap_or(0)
            .min(rows.len());
        if grown <= header {
            break;
        }
        header = grown;
    }
    grid.header_rows = header;
    grid.rows = rows;
    grid
}
