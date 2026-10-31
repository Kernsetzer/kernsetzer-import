//! Front matter: the dialect's `Key: value` lines at the top, or a YAML
//! block between `---` lines as Pandoc, Obsidian and iA Writer write it.
//! Both feed the same fields; keys are case-insensitive.
//!
//! ```text
//! ---                            <- YAML: `---` on the first line, closed by
//! title: "Slow Dough: A Guide"      `---` or `...`. Plain, quoted, `|` and `>`
//! author:                           values, lists (`- A` lines or `[A, B]`),
//!   - Ada Quill                     `#` comments. Other keys warn by name and
//! rights: © 2026 Ada Quill          are left out; so is a line that is not
//! date: 2026-03-14                  `key: value` (the block is still read).
//! ---
//! ```
//!
//! The parser reads only what the fields need, not all of YAML: nested
//! mappings (Pandoc's `author: - name: …`) are left out with a warning.

use crate::{ImportWarning, WarningDetail, WarningKind};
use setzer_model::{Lang, Metadata};

/// What the front matter says, and where the body starts.
#[derive(Default)]
pub(crate) struct FrontMatter {
    pub meta: Metadata,
    /// Whether a `Lang:` key set `meta.lang`; otherwise it is detected.
    pub lang_given: bool,
    pub dedication: Option<String>,
    /// Index of the first line after the front matter.
    pub end: usize,
    pub warnings: Vec<ImportWarning>,
}

/// A metadata field a front-matter key sets.
#[derive(Debug, Clone, Copy)]
enum Field {
    Title,
    Subtitle,
    Author,
    Lang,
    Publisher,
    Copyright,
    Dedication,
}

fn field(key: &str) -> Option<Field> {
    Some(match key.trim().to_ascii_lowercase().as_str() {
        "title" => Field::Title,
        "subtitle" => Field::Subtitle,
        "author" => Field::Author,
        "lang" | "language" => Field::Lang,
        "publisher" => Field::Publisher,
        // `rights` is Pandoc's name for the copyright line.
        "copyright" | "rights" => Field::Copyright,
        "dedication" => Field::Dedication,
        _ => return None,
    })
}

/// Whether `key` is the language key (`Lang`, `Language`): its value is a
/// language tag the importer keeps as written, not prose.
pub(crate) fn is_lang_key(key: &str) -> bool {
    matches!(field(key), Some(Field::Lang))
}

/// The known keys, for warnings.
const KEYS: &str = "title, subtitle, author, lang, publisher, copyright (or rights) and dedication";

/// Read the front matter at the top of `lines`: a YAML block if there is
/// one, else dialect lines.
pub(crate) fn read(lines: &[&str]) -> FrontMatter {
    yaml(lines).unwrap_or_else(|| dialect(lines))
}

impl FrontMatter {
    /// Set `field` from one value; an empty value sets nothing. Line breaks
    /// (from YAML `|`) are kept in the dedication only.
    fn set(&mut self, field: Field, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
        match field {
            Field::Title => self.meta.title = line,
            Field::Subtitle => self.meta.subtitle = Some(line),
            Field::Author => self.meta.authors.push(line),
            Field::Lang => {
                self.meta.lang = Lang(line);
                self.lang_given = true;
            }
            Field::Publisher => self.meta.publisher = Some(line),
            Field::Copyright => self.meta.copyright = Some(line),
            Field::Dedication => self.dedication = Some(value.to_string()),
        }
    }

    fn warn(&mut self, line_no: usize, key: Option<&str>, message: String) {
        self.warnings.push(ImportWarning {
            kind: WarningKind::FrontMatterKeyIgnored,
            location: line_no,
            section: None,
            detail: key.map(|k| WarningDetail::FrontMatterKey { key: k.into() }),
            message,
        });
    }
}

/// `Key: value` lines before the first blank line or heading. The first
/// line that is not a known key ends them: without a closing line, an
/// unknown key may well be the first line of the text ("Note: …").
fn dialect(lines: &[&str]) -> FrontMatter {
    let mut fm = FrontMatter::default();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        fm.end = i;
        if line.is_empty() || line.starts_with('#') {
            return fm;
        }
        match line
            .split_once(':')
            .and_then(|(key, value)| Some((field(key)?, value)))
        {
            Some((f, value)) => fm.set(f, value),
            None => return fm,
        }
    }
    fm.end = lines.len();
    fm
}

/// A YAML block: `---` on the first line, `key: value` lines right under
/// it, then `---` or `...`. `None` (so the dialect rules apply, and `---`
/// is a scene break) when the block is not closed, or does not start right
/// under the `---` with a `key: value` line (comments aside).
fn yaml(lines: &[&str]) -> Option<FrontMatter> {
    let first = lines.first()?.trim_start_matches('\u{feff}').trim_end();
    if first != "---" {
        return None;
    }
    let close = 1 + lines[1..]
        .iter()
        .position(|l| matches!(l.trim_end(), "---" | "..."))?;
    let block = &lines[1..close];
    // As in Pandoc, the block starts right under the `---`, so a scene
    // break with text after a blank line is no YAML.
    if block.first().is_none_or(|l| l.trim().is_empty()) {
        return None;
    }
    let opening = block.iter().find(|l| !l.trim_start().starts_with('#'))?;
    yaml_key(opening)?;

    let mut fm = FrontMatter {
        end: close + 1,
        ..FrontMatter::default()
    };
    let mut i = 0;
    while let Some(&line) = block.get(i) {
        let line_no = i + 2;
        // The value's further lines: indented ones, `- ` items, and blank
        // lines between them.
        let mut next = i + 1;
        while block
            .get(next)
            .is_some_and(|l| l.trim().is_empty() || continues_value(l))
        {
            next += 1;
        }
        while next > i + 1 && block[next - 1].trim().is_empty() {
            next -= 1;
        }
        let more = &block[i + 1..next];
        i = next;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, rest)) = yaml_key(line) else {
            fm.warn(
                line_no,
                None,
                format!(
                    "Line {line_no} of the front matter is not `key: value`, so it was left out \
                     of the book."
                ),
            );
            continue;
        };
        let Some(f) = field(key) else {
            fm.warn(
                line_no,
                Some(key),
                format!(
                    "Kernsetzer does not read the front-matter key `{key}`, so it was left out of \
                     the book. It reads {KEYS}."
                ),
            );
            continue;
        };
        match yaml_value(rest, more) {
            Some(values) if matches!(f, Field::Author) => {
                for v in &values {
                    fm.set(f, v);
                }
            }
            Some(values) => fm.set(f, &values.join(", ")),
            None => fm.warn(
                line_no,
                Some(key),
                format!(
                    "The front-matter value of `{key}` could not be read (Kernsetzer reads text and \
                     lists of text), so it was left out. Enter it in the book's details."
                ),
            ),
        }
    }
    Some(fm)
}

/// Whether a line under a key belongs to its value: indented, or a list
/// item (YAML allows `- A` at the key's own indentation).
fn continues_value(line: &str) -> bool {
    line.starts_with([' ', '\t']) || line.starts_with("- ") || line.trim_end() == "-"
}

/// A top-level `key: value` line → (key, the rest after the colon). The
/// colon ends the key only when a space or the line end follows, so
/// `https://…` is no key.
fn yaml_key(line: &str) -> Option<(&str, &str)> {
    if line.starts_with([' ', '\t', '-', '[', '{', '#']) {
        return None;
    }
    let colon = line.char_indices().find_map(|(i, c)| {
        (c == ':'
            && line[i + 1..]
                .chars()
                .next()
                .is_none_or(|n| n == ' ' || n == '\t'))
        .then_some(i)
    })?;
    let key = line[..colon].trim().trim_matches(['"', '\'']);
    (!key.is_empty() && key.chars().count() <= 60).then_some((key, &line[colon + 1..]))
}

/// The value after a key (`rest`) and its further lines (`more`) → its
/// strings: one for text, several for a list. `None` for a mapping, which
/// no field takes.
fn yaml_value(rest: &str, more: &[&str]) -> Option<Vec<String>> {
    let rest = rest.trim();
    if rest.starts_with(['|', '>']) {
        return Some(vec![block_scalar(rest.starts_with('|'), more)]);
    }
    if let Some(flow) = rest.strip_prefix('[') {
        let inner = flow.split_once(']').map_or(flow, |(inner, _)| inner);
        return Some(split_flow(inner).iter().map(|v| scalar(v)).collect());
    }
    if rest.is_empty() {
        let first = more.iter().map(|l| l.trim()).find(|l| !l.is_empty());
        return match first {
            None => Some(Vec::new()),
            Some(l) if l == "-" || l.starts_with("- ") => list_items(more),
            Some(l) if yaml_key(l).is_some() => None,
            Some(_) => Some(vec![scalar(&fold(more))]),
        };
    }
    let mut all = vec![rest];
    all.extend(more.iter().map(|l| l.trim()));
    Some(vec![scalar(&fold(&all))])
}

/// `- item` lines → the items; a line that is not an item continues the
/// one before it. `None` if an item is a mapping (`- name: …`).
fn list_items(lines: &[&str]) -> Option<Vec<String>> {
    let mut items: Vec<Vec<&str>> = Vec::new();
    for line in lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        match line
            .strip_prefix('-')
            .filter(|r| r.is_empty() || r.starts_with([' ', '\t']))
        {
            Some(item) => items.push(vec![item.trim()]),
            None => items.last_mut()?.push(line),
        }
    }
    items
        .iter()
        .map(|lines| {
            let text = fold(lines);
            yaml_key(&text).is_none().then(|| scalar(&text))
        })
        .collect()
}

/// A `|` (literal: lines kept) or `>` (folded: lines joined, a blank line
/// a line break) block scalar from its indented lines.
fn block_scalar(literal: bool, lines: &[&str]) -> String {
    let lines: Vec<&str> = lines.iter().map(|l| l.trim()).collect();
    if literal {
        return lines.join("\n");
    }
    lines
        .split(|l| l.is_empty())
        .filter(|p| !p.is_empty())
        .map(|p| p.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lines of one plain value joined with spaces (YAML folds them).
fn fold(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The items of a flow list's inside, `A, "B, Jr.", C`.
fn split_flow(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut quote = None;
    let mut start = 0;
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, ',') => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&inner[start..]);
    items.retain(|i| !i.trim().is_empty());
    items
}

/// One scalar: `"double"` (backslash escapes), `'single'` (`''` is a
/// quote) or plain, where ` #` starts a comment.
fn scalar(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                },
                _ => out.push(c),
            }
        }
        return out;
    }
    if let Some(rest) = s.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(c);
        }
        return out;
    }
    let end = s.find(" #").or_else(|| s.find("\t#")).unwrap_or(s.len());
    s[..end].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(src: &str) -> FrontMatter {
        read(&src.lines().collect::<Vec<_>>())
    }

    #[test]
    fn dialect_lines() {
        let f = fm("Title: T\nauthor: A\nRights: © B\nLang: de\nGenre: Roman\n\n# Eins\n");
        assert_eq!(f.meta.title, "T");
        assert_eq!(f.meta.authors, ["A"]);
        assert_eq!(f.meta.copyright.as_deref(), Some("© B"));
        assert_eq!(f.meta.lang.0, "de");
        assert!(f.lang_given);
        // An unknown key ends the dialect front matter: it is text.
        assert_eq!(f.end, 4);
        assert!(f.warnings.is_empty());
        assert_eq!(fm("Title: T").end, 1);
    }

    #[test]
    fn yaml_values() {
        let f = fm("\u{feff}---\n\
             title: 'It''s \"here\"' # comment\n\
             subtitle: \"Two\\nlines: \\\"quoted\\\"\"\n\
             author: [Elizabeth Bennet, \"Roe, Jr.\"]\n\
             publisher: Kurt\n  Wolff Verlag\n\
             dedication: >-\n  For A,\n  folded.\n\n  New line.\n\
             ...\nBody\n");
        assert_eq!(f.meta.title, "It's \"here\"");
        assert_eq!(f.meta.subtitle.as_deref(), Some("Two lines: \"quoted\""));
        assert_eq!(f.meta.authors, ["Elizabeth Bennet", "Roe, Jr."]);
        assert_eq!(f.meta.publisher.as_deref(), Some("Kurt Wolff Verlag"));
        assert_eq!(f.dedication.as_deref(), Some("For A, folded.\nNew line."));
        assert_eq!(f.end, 12);
        assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    }

    #[test]
    fn yaml_lists_and_problems() {
        let f = fm(
            "---\nauthor:\n- A\n-   B\n    continued\nlang:\nkeywords:\n  - x\nnot a key\nauthor:\n  - name: C\n    affiliation: D\n---\n",
        );
        assert_eq!(f.meta.authors, ["A", "B continued"]);
        assert!(!f.lang_given);
        let got: Vec<_> = f.warnings.iter().map(|w| w.location).collect();
        assert_eq!(got, [7, 9, 10]);
        assert!(f.warnings[0].message.contains("`keywords`"));
        assert!(f.warnings[2].message.contains("`author`"));
    }

    #[test]
    fn not_yaml() {
        // Unclosed, or not `key: value` under the opening line: the dialect
        // reads nothing, and `---` stays a scene break.
        for src in [
            "---\ntitle: T\n\n# Eins\n",
            "---\nA quote, not YAML.\n---\n",
            "---\n\n---\n",
            "Title: T\n---\n",
        ] {
            let f = fm(src);
            assert_eq!(f.end, usize::from(src.starts_with("Title")), "{src:?}");
            assert!(f.warnings.is_empty());
        }
    }
}
