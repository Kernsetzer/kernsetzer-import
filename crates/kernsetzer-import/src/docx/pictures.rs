//! Picture size and placement from Word. The parser reads
//! what Word wrote for each picture ([`Layout`]: `wp:extent`, the wrap
//! element, `wp:positionH`) and for each section ([`Page`]: `w:pgSz` and
//! `w:pgMar`); once the story is read, [`resolve`] turns the two into what
//! the model keeps ([`Place`]: a width, a placement and a side).
//!
//! Word keeps a picture's size in EMU (914,400 per inch) and a section's
//! page in twips (1,440 per inch); everything here is in EMU.

use super::story::{Para, Piece};
use crate::{ImportWarning, WarningKind};
use setzer_model::{ImagePlacement, ImageSide};

const EMU_PER_TWIP: i64 = 635;

/// A picture that fills this share of the text block's width and height,
/// alone in its paragraph, is a page of its own (percent).
const PAGE_FILL: i64 = 90;
/// A floating picture wider than this share of the text stays in the text:
/// no column of text fits beside it (percent).
const BESIDE_MAX: i64 = 60;

/// How Word lays a picture out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Wrap {
    /// `wp:inline`, or an anchor without a wrap element: in the line.
    #[default]
    Inline,
    /// `wp:wrapSquare`, `wp:wrapTight` and `wp:wrapThrough`: the text runs
    /// round the picture.
    Around,
    /// `wp:wrapTopAndBottom`: the picture stands alone between lines.
    TopBottom,
    /// `wp:wrapNone`: in front of or behind the text.
    Over,
}

/// What Word wrote about one picture (EMU; 0 when absent).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Layout {
    pub cx: i64,
    pub cy: i64,
    pub wrap: Wrap,
    /// `wp:positionH`: what the offset counts from (`relativeFrom`).
    pub h_from: Option<String>,
    /// `wp:align`: left, right, center, inside or outside.
    pub h_align: Option<String>,
    /// `wp:posOffset`.
    pub h_offset: Option<i64>,
}

/// What the model keeps of a picture's layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Place {
    pub width: Option<u8>,
    pub placement: ImagePlacement,
    pub side: ImageSide,
}

/// A section's page, read from its `w:sectPr` (twips).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Sect {
    pub w: Option<i64>,
    pub h: Option<i64>,
    pub left: Option<i64>,
    pub right: Option<i64>,
    pub top: Option<i64>,
    pub bottom: Option<i64>,
    pub gutter: Option<i64>,
}

impl Sect {
    /// The text block. Word's own defaults (US Letter, one-inch margins)
    /// stand in for what the section leaves out, and for a page whose
    /// margins leave no text.
    pub fn page(&self) -> Page {
        let width = self.w.unwrap_or(12240);
        let height = self.h.unwrap_or(15840);
        let left = self.left.unwrap_or(1440);
        let text_w = width - left - self.right.unwrap_or(1440) - self.gutter.unwrap_or(0);
        let text_h = height - self.top.unwrap_or(1440) - self.bottom.unwrap_or(1440);
        if text_w <= 0 || text_h <= 0 {
            return Page::default();
        }
        Page {
            text_w: text_w * EMU_PER_TWIP,
            text_h: text_h * EMU_PER_TWIP,
            left: left * EMU_PER_TWIP,
        }
    }
}

/// The text block of a section (EMU) and where it starts on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Page {
    pub text_w: i64,
    pub text_h: i64,
    pub left: i64,
}

impl Default for Page {
    /// No section properties: US Letter with one-inch margins.
    fn default() -> Self {
        Page {
            text_w: 6 * 914_400 + 457_200,
            text_h: 9 * 914_400,
            left: 914_400,
        }
    }
}

/// Fill in [`Place`] for every picture of the story's paragraphs, and warn
/// once if Word had pictures in front of or behind the text. `pages` is
/// the text block of each section, in order; `Para::section` indexes it.
/// Pictures in footnotes and endnotes, and in tables (a table row is joined
/// into one paragraph and its cells are not the page), keep Word's size
/// out of the book: they stay in the text at their own size.
pub(super) fn resolve(paras: &mut [Para], pages: &[Page], warnings: &mut Vec<ImportWarning>) {
    let mut dropped = 0usize;
    let mut first = 0usize;
    for p in paras.iter_mut() {
        if p.note.is_some() || p.in_table {
            continue;
        }
        let page = pages.get(p.section).copied().unwrap_or_default();
        let alone = alone_picture(p);
        for s in &mut p.segs {
            let Piece::Image { layout, place, .. } = &mut s.piece else {
                continue;
            };
            if layout.wrap == Wrap::Over {
                if dropped == 0 {
                    first = p.index;
                }
                dropped += 1;
            }
            *place = place_of(layout, &page, alone);
        }
    }
    if dropped > 0 {
        let (count, what, each) = if dropped == 1 {
            (String::new(), "a picture was", "it")
        } else {
            (format!("{dropped} "), "pictures were", "each")
        };
        warnings.push(ImportWarning {
            kind: WarningKind::ImagePositionDropped,
            location: first,
            section: None,
            message: format!(
                "Paragraph {first}: {count}{what} placed in front of or behind the text in Word. \
                 Kernsetzer sets {each} in the text where it was anchored; the position is not \
                 kept."
            ),
            detail: None,
        });
    }
}

/// Whether the paragraph shows one picture and nothing else.
fn alone_picture(p: &Para) -> bool {
    let mut pictures = 0;
    for s in &p.segs {
        match &s.piece {
            Piece::Image { .. } => pictures += 1,
            Piece::Text(t) if t.trim().is_empty() => {}
            Piece::LineBreak => {}
            _ => return false,
        }
    }
    pictures == 1
}

fn place_of(l: &Layout, page: &Page, alone: bool) -> Place {
    let width = (l.cx > 0).then(|| round_width(l.cx, page.text_w));
    let in_text = Place {
        width,
        ..Place::default()
    };
    match l.wrap {
        Wrap::Inline | Wrap::TopBottom if alone && fills(l, page) => Place {
            placement: ImagePlacement::Page,
            ..Place::default()
        },
        Wrap::Inline | Wrap::TopBottom | Wrap::Over => in_text,
        Wrap::Around => {
            let too_wide = l.cx * 100 > BESIDE_MAX * page.text_w;
            let side = side_of(l, page);
            match side {
                Some(side) if !too_wide => Place {
                    width,
                    placement: ImagePlacement::Beside,
                    side,
                },
                _ => in_text,
            }
        }
    }
}

/// The width as a percentage of the text block, to the nearest 5 and
/// within 10–100.
fn round_width(cx: i64, text_w: i64) -> u8 {
    let percent = cx as f64 * 100.0 / text_w as f64;
    let rounded = (percent / 5.0).round() as i64 * 5;
    rounded.clamp(10, 100) as u8
}

fn fills(l: &Layout, page: &Page) -> bool {
    l.cx * 100 >= PAGE_FILL * page.text_w && l.cy * 100 >= PAGE_FILL * page.text_h
}

/// Which side a floating picture stands on, from `wp:positionH`; `None`
/// when it is centred (text on both sides is not a wrap we can set).
fn side_of(l: &Layout, page: &Page) -> Option<ImageSide> {
    match l.h_align.as_deref() {
        Some("left") => return Some(ImageSide::Left),
        Some("right") => return Some(ImageSide::Right),
        Some("inside") => return Some(ImageSide::Inside),
        Some("outside") => return Some(ImageSide::Outside),
        Some("center") => return None,
        _ => {}
    }
    match (l.h_from.as_deref(), l.h_offset) {
        (Some("leftMargin"), _) => Some(ImageSide::Left),
        (Some("rightMargin"), _) => Some(ImageSide::Right),
        (from, Some(offset)) => {
            // The offset counts from the column (or margin, or the
            // character); from the page it includes the left margin.
            let x = if from == Some("page") {
                offset - page.left
            } else {
                offset
            };
            // The picture's middle decides: a picture reaching across the
            // column's middle from the left edge is still a left picture.
            let middle = x + l.cx / 2;
            Some(if middle * 2 < page.text_w {
                ImageSide::Left
            } else {
                ImageSide::Right
            })
        }
        // Word's default for an anchor with no horizontal position is the
        // right edge of the column.
        (_, None) => Some(ImageSide::Right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: i64 = 914_400;

    fn layout(w_in: f64, h_in: f64, wrap: Wrap) -> Layout {
        Layout {
            cx: (w_in * IN as f64) as i64,
            cy: (h_in * IN as f64) as i64,
            wrap,
            ..Layout::default()
        }
    }

    #[test]
    fn widths_round_to_five_within_ten_and_a_hundred() {
        let w = 6 * IN + IN / 2;
        let at = |inches: f64| round_width((inches * IN as f64) as i64, w);
        assert_eq!(at(6.5), 100);
        assert_eq!(at(7.0), 100);
        assert_eq!(at(2.0), 30);
        assert_eq!(at(1.0), 15);
        assert_eq!(at(0.2), 10);
        assert_eq!(at(3.25), 50);
        // 2.75 percent rounds to 5, which is raised to the minimum.
        assert_eq!(round_width(w * 275 / 10_000, w), 10);
    }

    #[test]
    fn section_text_block() {
        let s = Sect {
            w: Some(11906),
            h: Some(16838),
            left: Some(1417),
            right: Some(1417),
            top: Some(1417),
            bottom: Some(1417),
            gutter: Some(284),
        };
        let p = s.page();
        assert_eq!(p.text_w, (11906 - 1417 * 2 - 284) * EMU_PER_TWIP);
        assert_eq!(p.text_h, (16838 - 1417 * 2) * EMU_PER_TWIP);
        assert_eq!(p.left, 1417 * EMU_PER_TWIP);
        assert_eq!(Sect::default().page(), Page::default());
        // Margins that leave no text fall back to the default page.
        let bad = Sect {
            w: Some(1000),
            left: Some(900),
            right: Some(900),
            ..Sect::default()
        };
        assert_eq!(bad.page(), Page::default());
    }

    #[test]
    fn sides() {
        let page = Page::default();
        let with = |from: Option<&str>, align: Option<&str>, offset: Option<f64>| {
            let l = Layout {
                h_from: from.map(Into::into),
                h_align: align.map(Into::into),
                h_offset: offset.map(|o| (o * IN as f64) as i64),
                ..layout(2.0, 1.0, Wrap::Around)
            };
            place_of(&l, &page, false)
        };
        let beside = |side| Place {
            width: Some(30),
            placement: ImagePlacement::Beside,
            side,
        };
        assert_eq!(with(None, Some("left"), None), beside(ImageSide::Left));
        assert_eq!(with(None, Some("inside"), None), beside(ImageSide::Inside));
        assert_eq!(
            with(None, Some("outside"), None),
            beside(ImageSide::Outside)
        );
        assert_eq!(
            with(Some("margin"), None, Some(0.0)),
            beside(ImageSide::Left)
        );
        // Middle at 2.4 + 1 = 3.4 in of 6.5: the right half.
        assert_eq!(
            with(Some("column"), None, Some(2.4)),
            beside(ImageSide::Right)
        );
        // Middle at 2.2 + 1 = 3.2 in: the left half, though it ends right of it.
        assert_eq!(
            with(Some("column"), None, Some(2.2)),
            beside(ImageSide::Left)
        );
        // From the page edge, the one-inch margin is taken off.
        assert_eq!(with(Some("page"), None, Some(1.2)), beside(ImageSide::Left));
        assert_eq!(
            with(Some("page"), None, Some(5.0)),
            beside(ImageSide::Right)
        );
        assert_eq!(
            with(Some("leftMargin"), None, Some(5.0)),
            beside(ImageSide::Left)
        );
        assert_eq!(with(None, None, None), beside(ImageSide::Right));
        // Centred: text on both sides, which is not a wrap we set.
        assert_eq!(
            with(None, Some("center"), None),
            Place {
                width: Some(30),
                ..Place::default()
            }
        );
    }

    #[test]
    fn wide_floats_and_other_wraps_stay_in_the_text() {
        let page = Page::default();
        let at = |w: f64, wrap| place_of(&layout(w, 2.0, wrap), &page, false);
        assert_eq!(at(3.9, Wrap::Around).placement, ImagePlacement::Beside);
        // 60 percent of 6.5 in is 3.9 in; a hair more stays in the text.
        assert_eq!(at(3.95, Wrap::Around).placement, ImagePlacement::Inline);
        assert_eq!(at(3.95, Wrap::Around).width, Some(60));
        assert_eq!(at(2.0, Wrap::TopBottom).placement, ImagePlacement::Inline);
        assert_eq!(at(2.0, Wrap::Over).placement, ImagePlacement::Inline);
        assert_eq!(at(2.0, Wrap::Over).width, Some(30));
    }

    #[test]
    fn page_needs_both_ways_and_a_paragraph_of_its_own() {
        let page = Page::default();
        let at = |w, h, alone| place_of(&layout(w, h, Wrap::Inline), &page, alone);
        assert_eq!(at(5.85, 8.1, true).placement, ImagePlacement::Page);
        assert_eq!(at(5.85, 8.1, true).width, None);
        assert_eq!(at(5.84, 8.1, true).placement, ImagePlacement::Inline);
        assert_eq!(at(5.85, 8.09, true).placement, ImagePlacement::Inline);
        assert_eq!(at(6.5, 9.0, false).placement, ImagePlacement::Inline);
        assert_eq!(at(6.5, 9.0, false).width, Some(100));
        // A floating picture is never a page, whatever its size.
        let l = layout(6.5, 9.0, Wrap::Around);
        assert_eq!(place_of(&l, &page, true).placement, ImagePlacement::Inline);
    }
}
