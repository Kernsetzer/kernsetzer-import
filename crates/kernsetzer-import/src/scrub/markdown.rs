//! The scrambled copy of a Markdown manuscript.
//!
//! Only prose changes. Kept as written: the front matter's keys and its
//! language, `:::` fence lines, `\newpage`, the `Table:` of a caption line,
//! footnote labels (`[^1]`), link and image targets (the path, not the title
//! or caption), autolinks (`<https://…>`), Pandoc attributes (`{-}`,
//! `{.smallcaps}`, `{width=50%}`) and, as everywhere, every non-letter: `#`,
//! `>`, list markers, emphasis marks, table pipes, line endings and byte
//! order mark.
//!
//! The front matter's extent comes from the importer's own reader, so the
//! two cannot disagree about where the book starts.

use super::words::{Context, LANG_BUDGET, Scrambler, seed_of};
use super::{ScrubError, image};
use crate::markdown::front_matter;

/// A scrambled Markdown manuscript and the blank pictures it refers to.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrubbedMarkdown {
    pub text: String,
    /// `(path as written in the manuscript, blank picture)` for each
    /// picture the importer asked for and the caller had. The copy refers
    /// to these paths; a writer puts them next to the copy (and must not
    /// follow a path out of the folder: `..`, an absolute path).
    pub images: Vec<(String, Vec<u8>)>,
}

pub(super) fn scrub(
    bytes: &[u8],
    mut pictures: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<ScrubbedMarkdown, ScrubError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ScrubError::NotUtf8)?;
    // Windows editors start UTF-8 files with a byte order mark; the
    // importer reads past it, and the copy keeps it.
    let (bom, src) = match text.strip_prefix('\u{feff}') {
        Some(rest) => ("\u{feff}", rest),
        None => ("", text),
    };
    let comments_out = crate::markdown::strip_comments(src);
    let lines: Vec<&str> = comments_out.lines().collect();
    let pieces: Vec<&str> = src.split_inclusive('\n').collect();
    // A comment alone on the last line, with no line end after it, leaves
    // nothing there once stripped: one piece more than lines, and that
    // piece is the comment.
    let comment_tail =
        pieces.len() == lines.len() + 1 && comments_out.ends_with('\n') && !src.ends_with('\n');
    if pieces.len() != lines.len() && !comment_tail {
        return Err(ScrubError::Part {
            part: "the text".into(),
            why: "its lines could not be told apart".into(),
        });
    }
    let (front_end, lang_given) = if comments_out.trim().is_empty() {
        (0, false)
    } else {
        let front = front_matter::read(&lines);
        (front.end, front.lang_given)
    };
    let units = units(&pieces, front_end);

    let sc = Scrambler::new(seed_of(bytes));
    let mut out = String::with_capacity(text.len());
    out.push_str(bom);
    // The importer counts the function words of the body's first words to
    // tell the language, unless the front matter says it.
    sc.set_lang_budget(0);
    for (i, piece) in pieces.iter().enumerate() {
        let body = piece.strip_suffix('\n').unwrap_or(piece);
        let ending = &piece[body.len()..];
        let new = if i < front_end {
            front_matter_line(&sc, body)
        } else {
            if i == front_end {
                sc.set_lang_budget(if lang_given { 0 } else { LANG_BUDGET });
            }
            body_line(&sc, body, units[i])
        };
        out.push_str(&new);
        out.push_str(ending);
    }

    // The pictures: ask the way the importer does, so the copy has
    // exactly the pictures the original's import found.
    let mut images = Vec::new();
    let mut failed = None;
    let _ = crate::markdown::import(src, |path| {
        let bytes = pictures(path)?;
        match image::blank(&bytes) {
            Ok(blank) => images.push((path.to_string(), blank)),
            Err(why) => {
                failed.get_or_insert((path.to_string(), why));
            }
        }
        Some(bytes)
    });
    if let Some((name, why)) = failed {
        return Err(ScrubError::Picture { name, why });
    }
    Ok(ScrubbedMarkdown { text: out, images })
}

/// A front-matter line: the key stays, the value is prose (except the
/// language). A comment, a list item or a continuation is prose whole.
fn front_matter_line(sc: &Scrambler, line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed == "---" || trimmed == "..." {
        return line.to_string();
    }
    let indented = line.starts_with([' ', '\t']);
    if !indented
        && !trimmed.starts_with(['#', '-'])
        && let Some((key, value)) = line.split_once(':')
    {
        if front_matter::is_lang_key(key.trim().trim_matches(['"', '\''])) {
            // The tag stays; a YAML comment after it is prose.
            return match value.find(" #") {
                Some(at) => format!(
                    "{key}:{}{}",
                    &value[..at],
                    scramble(sc, &value[at..], false)
                ),
                None => line.to_string(),
            };
        }
        return format!("{key}:{}", scramble(sc, value, false));
    }
    scramble(sc, line, false)
}

/// Where a line of the book stands in its paragraph.
#[derive(Debug, Clone, Copy, Default)]
struct Unit {
    /// The line begins a paragraph, a heading, a fence or a list item.
    start: bool,
    /// How many words the paragraph has, over all its source lines.
    words: usize,
}

/// The paragraphs of the book, line by line (index = line; the front
/// matter's lines have none). A hard-wrapped paragraph is one paragraph:
/// the importer reads it whole, so a source line is no unit to judge
/// "short" by. A heading, a fence line and a list item begin a paragraph
/// of their own; a line right after a heading, a fence or the front matter
/// begins one too, blank line or not.
fn units(pieces: &[&str], front_end: usize) -> Vec<Unit> {
    let n = pieces.len();
    let mut first_of: Vec<Option<usize>> = vec![None; n];
    let mut total = vec![0usize; n];
    let mut open: Option<usize> = None; // the paragraph the next line may join
    for i in front_end..n {
        let t = pieces[i].trim();
        if t.is_empty() {
            open = None;
            continue;
        }
        let alone = is_alone(t);
        let first = match open {
            Some(f) if !alone && !is_list_item(t) => f,
            _ => i,
        };
        first_of[i] = Some(first);
        total[first] += t
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .count();
        open = (!alone).then_some(first);
    }
    (0..n)
        .map(|i| match first_of[i] {
            Some(f) => Unit {
                start: f == i,
                words: total[f],
            },
            None => Unit::default(),
        })
        .collect()
}

/// A line that is a paragraph of its own: a heading, a fence, a page break.
fn is_alone(t: &str) -> bool {
    t.starts_with('#') || t.starts_with(":::") || matches!(t, "\\newpage" | "\\pagebreak")
}

/// `- item`, `* item`, `+ item`, `3. item`, `3) item`.
fn is_list_item(t: &str) -> bool {
    let after_marker = |rest: &str| rest.starts_with([' ', '\t']);
    if let Some(rest) = t.strip_prefix(['-', '*', '+']) {
        return after_marker(rest);
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && t[digits..]
            .strip_prefix(['.', ')'])
            .is_some_and(after_marker)
}

/// A line of the book.
fn body_line(sc: &Scrambler, line: &str, unit: Unit) -> String {
    let trimmed = line.trim();
    // `:::verse`, `:::`; `\newpage`, `\pagebreak`.
    if trimmed.starts_with(":::") || matches!(trimmed, "\\newpage" | "\\pagebreak") {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut protect = protected(&chars);
    keep_caption_marker(&chars, &mut protect);
    let cx = Context {
        para_start: unit.start,
        continuation: !unit.start,
        words: Some(unit.words),
    };
    sc.scramble_in(&chars, &protect, cx).into_iter().collect()
}

/// The `Table:` that starts a Pandoc caption line (`Table: text`) is a word
/// the importer reads, so it stays.
fn keep_caption_marker(line: &[char], keep: &mut [bool]) {
    const MARKER: [char; 6] = ['T', 'a', 'b', 'l', 'e', ':'];
    let Some(start) = line.iter().position(|c| !c.is_whitespace()) else {
        return;
    };
    let after = start + MARKER.len();
    if line[start..].starts_with(&MARKER)
        && line.get(after).is_some_and(|c| matches!(c, ' ' | '\t'))
    {
        keep[start..after].fill(true);
    }
}

fn scramble(sc: &Scrambler, text: &str, para_start: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let protect = vec![false; chars.len()];
    sc.scramble(&chars, &protect, para_start)
        .into_iter()
        .collect()
}

/// Which chars of a line are syntax or addresses, not prose.
fn protected(line: &[char]) -> Vec<bool> {
    let n = line.len();
    let mut keep = vec![false; n];
    let find = |from: usize, c: char| {
        line[from.min(n)..]
            .iter()
            .position(|&x| x == c)
            .map(|p| from + p)
    };
    let mut i = 0;
    while i < n {
        match line[i] {
            // [^label]
            '[' if line.get(i + 1) == Some(&'^') => {
                if let Some(j) = find(i + 2, ']') {
                    keep[i..=j].fill(true);
                    i = j + 1;
                    continue;
                }
            }
            // ](target "title"): the target, not the title
            ']' if line.get(i + 1) == Some(&'(') => {
                let end = target_end(line, i + 2);
                keep[i + 2..end].fill(true);
                i = end;
                continue;
            }
            // <https://…>, <mailto:…>; not a comment
            '<' => {
                if let Some(j) = find(i + 1, '>') {
                    let inner = &line[i + 1..j];
                    if !inner.iter().any(|c| c.is_whitespace())
                        && inner.contains(&':')
                        && inner.first() != Some(&'!')
                    {
                        keep[i..=j].fill(true);
                        i = j + 1;
                        continue;
                    }
                }
            }
            // {-}, {.smallcaps}, {#id}, {width=50%}
            '{' => {
                if let Some(j) = find(i + 1, '}') {
                    let inner = &line[i + 1..j];
                    if inner.first().is_some_and(|c| matches!(c, '.' | '#' | '-'))
                        || inner.contains(&'=')
                    {
                        keep[i..=j].fill(true);
                        i = j + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    keep
}

/// Where the target of a link or image that starts at `start` ends: at the
/// `>` of `<…>`, before a quoted title, or at the closing parenthesis.
/// Paths may contain spaces and parentheses.
fn target_end(line: &[char], start: usize) -> usize {
    if line.get(start) == Some(&'<')
        && let Some(p) = line[start..].iter().position(|&c| c == '>')
    {
        return start + p + 1;
    }
    let mut depth = 0usize;
    for (k, &c) in line.iter().enumerate().skip(start) {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return k,
            ')' => depth -= 1,
            '"' if k > start && matches!(line[k - 1], ' ' | '\t') => return k - 1,
            _ => {}
        }
    }
    line.len()
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
