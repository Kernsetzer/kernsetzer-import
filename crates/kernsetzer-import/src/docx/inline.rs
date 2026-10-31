//! Segments of one paragraph → model `Inline`s: manual-formatting cleanup
//! (leading tabs, double spaces, zero-width spaces), whitespace moved out of
//! emphasis, adjacent runs of equal formatting merged, and nesting in a
//! fixed order (link > super/subscript > strong > emphasis > small caps).

use super::story::{Fmt, Piece, Seg};
use setzer_model::Inline;

/// Clean up a paragraph's segments in place.
pub(super) fn normalize(segs: &mut Vec<Seg>) {
    for s in segs.iter_mut() {
        if let Piece::Text(t) = &mut s.piece {
            *t = clean_chars(t);
        }
    }
    collapse_whitespace(segs);
    segs.retain(|s| !matches!(&s.piece, Piece::Text(t) if t.is_empty()));
    detach_whitespace(segs);
    merge(segs);
}

/// Drop zero-width spaces and control characters; tabs and odd spaces
/// become plain spaces (collapsed later). No-break spaces are kept, and so
/// are soft hyphens (U+00AD): the author's own hyphenation points.
fn clean_chars(t: &str) -> String {
    t.chars()
        .filter_map(|c| match c {
            '\u{200B}' | '\u{FEFF}' => None,
            '\t' | '\n' | '\r' | '\u{000B}' | '\u{2028}' | '\u{2029}' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

fn is_breaking_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{00A0}' | '\u{202F}' | '\u{2007}')
}

/// One space between words; none at the start or end of a line. A space
/// typed before a footnote mark moves behind it ("word ¹" → "word¹ ").
fn collapse_whitespace(segs: &mut [Seg]) {
    let mut line_start = true;
    let mut prev_space = false;
    let mut space_after_note = false;
    let mut last_text: Option<usize> = None;
    for i in 0..segs.len() {
        match &segs[i].piece {
            Piece::Text(t) => {
                let mut out = String::with_capacity(t.len());
                for c in t.chars() {
                    if is_breaking_space(c) {
                        if !line_start && !prev_space {
                            out.push(' ');
                            prev_space = true;
                        }
                    } else {
                        if space_after_note {
                            out.push(' ');
                        }
                        out.push(c);
                        prev_space = false;
                        line_start = false;
                    }
                    space_after_note = false;
                }
                segs[i].piece = Piece::Text(out);
                last_text = Some(i);
            }
            Piece::LineBreak | Piece::Image { .. } => {
                trim_end_at(segs, last_text);
                line_start = true;
                prev_space = false;
                space_after_note = false;
            }
            Piece::Note(_) => {
                if prev_space {
                    trim_end_at(segs, last_text);
                    prev_space = false;
                    space_after_note = true;
                }
            }
        }
    }
    trim_end_at(segs, last_text);
}

fn trim_end_at(segs: &mut [Seg], i: Option<usize>) {
    if let Some(i) = i
        && let Piece::Text(t) = &mut segs[i].piece
    {
        let n = t.trim_end_matches(is_breaking_space).len();
        t.truncate(n);
    }
}

/// Split leading/trailing spaces off formatted segments and give them only
/// the formatting both neighbours share, so `*word *next` becomes
/// `*word* next`.
fn detach_whitespace(segs: &mut Vec<Seg>) {
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len() + 4);
    for s in segs.drain(..) {
        let Piece::Text(t) = &s.piece else {
            out.push(s);
            continue;
        };
        if s.fmt == Fmt::default() && s.link.is_none() {
            out.push(s);
            continue;
        }
        let core = t.trim_matches(' ');
        let lead = t.len() - t.trim_start_matches(' ').len();
        let trail = t.len() - t.trim_end_matches(' ').len();
        let ws = |s: &Seg| Seg {
            piece: Piece::Text(" ".into()),
            ..s.clone()
        };
        if core.is_empty() {
            out.push(ws(&s));
            continue;
        }
        if lead > 0 {
            out.push(ws(&s));
        }
        out.push(Seg {
            piece: Piece::Text(core.to_string()),
            ..s.clone()
        });
        if trail > 0 {
            out.push(ws(&s));
        }
    }
    // Whitespace-only segments take the formatting common to neighbours.
    let n = out.len();
    for i in 0..n {
        if !matches!(&out[i].piece, Piece::Text(t) if t.trim().is_empty()) {
            continue;
        }
        let neighbour = |j: Option<usize>| {
            j.and_then(|j| out.get(j))
                .filter(|s| matches!(s.piece, Piece::Text(_)))
                .map(|s| (s.fmt, s.link.clone()))
                .unwrap_or_default()
        };
        let (lf, ll) = neighbour(i.checked_sub(1));
        let (rf, rl) = neighbour(Some(i + 1));
        out[i].fmt = lf.common(rf);
        out[i].link = if ll == rl { ll } else { None };
        out[i].caps = false;
    }
    *segs = out;
}

/// Merge adjacent text segments with identical formatting.
fn merge(segs: &mut Vec<Seg>) {
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    for s in segs.drain(..) {
        if let (Some(last), Piece::Text(t)) = (out.last_mut(), &s.piece)
            && last.fmt == s.fmt
            && last.link == s.link
            && let Piece::Text(lt) = &mut last.piece
        {
            lt.push_str(t);
            last.caps &= s.caps;
            last.size = match (last.size, s.size) {
                (Some(a), Some(b)) => Some(a.min(b)),
                _ => None,
            };
            continue;
        }
        out.push(s);
    }
    *segs = out;
}

/// Plain text of segments (line breaks as spaces, notes omitted).
pub(super) fn plain_text(segs: &[Seg]) -> String {
    let mut out = String::new();
    for s in segs {
        match &s.piece {
            Piece::Text(t) => out.push_str(t),
            Piece::LineBreak => out.push(' '),
            _ => {}
        }
    }
    out
}

/// Nesting order: the outermost attribute first.
const DEPTH: usize = 6;

fn key(s: &Seg, depth: usize) -> Option<&str> {
    let on = |b: bool| b.then_some("");
    match depth {
        0 => s.link.as_deref(),
        1 => on(s.fmt.sup),
        2 => on(s.fmt.sub),
        3 => on(s.fmt.strong),
        4 => on(s.fmt.em),
        _ => on(s.fmt.small_caps),
    }
}

/// Build nested inlines from normalized segments.
pub(super) fn to_inlines(segs: &[Seg]) -> Vec<Inline> {
    build(segs, 0)
}

fn build(segs: &[Seg], depth: usize) -> Vec<Inline> {
    if depth == DEPTH {
        return leaves(segs);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < segs.len() {
        let k = key(&segs[i], depth);
        let mut j = i + 1;
        while j < segs.len() && key(&segs[j], depth) == k {
            j += 1;
        }
        let inner = build(&segs[i..j], depth + 1);
        match k {
            None => out.extend(inner),
            Some(url) => out.push(match depth {
                0 => Inline::Link {
                    url: url.to_string(),
                    content: inner,
                },
                1 => Inline::Superscript { content: inner },
                2 => Inline::Subscript { content: inner },
                3 => Inline::Strong { content: inner },
                4 => Inline::Emphasis { content: inner },
                _ => Inline::SmallCaps { content: inner },
            }),
        }
        i = j;
    }
    out
}

fn leaves(segs: &[Seg]) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for s in segs {
        match &s.piece {
            Piece::Text(t) => {
                if let Some(Inline::Text { text }) = out.last_mut() {
                    text.push_str(t);
                } else {
                    out.push(Inline::text(t.clone()));
                }
            }
            Piece::LineBreak => out.push(Inline::LineBreak),
            Piece::Note(blocks) => out.push(Inline::Footnote {
                blocks: blocks.clone(),
            }),
            Piece::Image { .. } => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(t: &str, fmt: Fmt) -> Seg {
        Seg {
            fmt,
            link: None,
            caps: false,
            size: None,
            piece: Piece::Text(t.into()),
        }
    }
    const P: Fmt = Fmt {
        em: false,
        strong: false,
        small_caps: false,
        sup: false,
        sub: false,
    };
    const EM: Fmt = Fmt { em: true, ..P };
    const EMB: Fmt = Fmt {
        em: true,
        strong: true,
        ..P
    };

    fn em(c: Vec<Inline>) -> Inline {
        Inline::Emphasis { content: c }
    }

    #[test]
    fn manual_formatting_is_cleaned() {
        let mut s = vec![seg("\t  Er  ging.  Dann\u{200B}ach  ", P)];
        normalize(&mut s);
        assert_eq!(plain_text(&s), "Er ging. Dannach");
    }

    #[test]
    fn soft_hyphens_are_kept() {
        let mut s = vec![seg("Ver\u{00AD}legen", P), seg("\u{00AD}heit", P)];
        normalize(&mut s);
        assert_eq!(plain_text(&s), "Ver\u{00AD}legen\u{00AD}heit");
    }

    #[test]
    fn whitespace_moves_out_of_emphasis() {
        let mut s = vec![seg("a", P), seg(" word ", EM), seg("next", P)];
        normalize(&mut s);
        assert_eq!(
            to_inlines(&s),
            vec![
                Inline::text("a "),
                em(vec![Inline::text("word")]),
                Inline::text(" next")
            ]
        );
    }

    #[test]
    fn nesting_order_is_fixed() {
        let mut s = vec![seg("one ", EM), seg("two", EMB), seg(" three", EM)];
        normalize(&mut s);
        assert_eq!(
            to_inlines(&s),
            vec![
                em(vec![Inline::text("one ")]),
                Inline::Strong {
                    content: vec![em(vec![Inline::text("two")])]
                },
                em(vec![Inline::text(" three")]),
            ],
            "strong is outside emphasis by the fixed order"
        );
        let mut s = vec![seg("ein", EM), seg("mal", EM)];
        normalize(&mut s);
        assert_eq!(to_inlines(&s), vec![em(vec![Inline::text("einmal")])]);
    }

    #[test]
    fn space_before_note_moves_after_it() {
        let note = Seg {
            fmt: P,
            link: None,
            caps: false,
            size: None,
            piece: Piece::Note(vec![]),
        };
        let mut s = vec![seg("word ", P), note, seg("more", P)];
        normalize(&mut s);
        assert_eq!(
            to_inlines(&s),
            vec![
                Inline::text("word"),
                Inline::Footnote { blocks: vec![] },
                Inline::text(" more")
            ]
        );
    }

    #[test]
    fn line_breaks_trim_around() {
        let br = Seg {
            fmt: P,
            link: None,
            caps: false,
            size: None,
            piece: Piece::LineBreak,
        };
        let mut s = vec![seg("Zeile eins ", P), br, seg("  zwei", P)];
        normalize(&mut s);
        assert_eq!(
            to_inlines(&s),
            vec![
                Inline::text("Zeile eins"),
                Inline::LineBreak,
                Inline::text("zwei")
            ]
        );
    }
}
