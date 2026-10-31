//! What heading text says about a section, shared by all importers:
//! section kind (Prolog, Danksagung, …), explicit chapter numbers
//! ("Chapter 3", "Kapitel drei", "Erstes Kapitel", "IV.") and scene-break
//! ornaments.

use crate::vocab;
use setzer_model::{Numbering, SectionKind};

/// A heading split into what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadingInfo {
    pub kind: SectionKind,
    /// Number written in the heading ("Chapter 3" → 3).
    pub number: Option<u32>,
    /// What remains after the label and number, if anything.
    pub title: Option<String>,
}

impl HeadingInfo {
    /// Model numbering for a section opened by this heading: the number
    /// written in a chapter or part heading, else chapters count on.
    pub fn numbering(&self) -> Numbering {
        match (self.kind, self.number) {
            (SectionKind::Chapter | SectionKind::Part, Some(n)) => Numbering::Explicit(n),
            (SectionKind::Chapter, None) => Numbering::Auto,
            _ => Numbering::None,
        }
    }

    /// Whether the heading is only a label ("Chapter 3", "IV") and the
    /// title, if any, is on the next line.
    pub fn is_bare_label(&self) -> bool {
        self.number.is_some() && self.title.is_none()
    }

    /// Whether the text could be a heading on its own, ignoring layout: a
    /// known section name or a chapter/part label. Used when a manuscript
    /// has no heading styles.
    pub fn is_label(&self) -> bool {
        self.kind != SectionKind::Chapter || self.number.is_some()
    }
}

/// Classify heading text. Never fails: unknown text is a chapter title.
/// Soft hyphens are dropped from headings: a label would hide behind one
/// ("Kapi\u{ad}tel 3"), and running heads and contents are better without.
pub(crate) fn classify(text: &str) -> HeadingInfo {
    let bare = text.replace('\u{ad}', "");
    let t = bare.trim();
    let key = t
        .trim_end_matches([':', '.'])
        .trim()
        .to_lowercase()
        .replace('\u{a0}', " ");

    if let Some(kind) = named_section(&key) {
        return HeadingInfo {
            kind,
            number: None,
            title: Some(t.to_string()),
        };
    }
    if let Some((kind, number, rest)) = labelled(t) {
        return HeadingInfo {
            kind,
            number: Some(number),
            title: rest,
        };
    }
    // "3", "3.", "IV", "IV." alone, or "3. Title" / "IV. Title".
    if let Some((number, rest)) = numbered(t) {
        return HeadingInfo {
            kind: SectionKind::Chapter,
            number: Some(number),
            title: rest,
        };
    }
    HeadingInfo {
        kind: SectionKind::Chapter,
        number: None,
        title: Some(t.to_string()),
    }
}

fn named_section(key: &str) -> Option<SectionKind> {
    if let Some((kind, _)) = vocab::SECTION_NAMES
        .iter()
        .find(|(_, names)| names.contains(&key))
    {
        return Some(*kind);
    }
    vocab::ALSO_BY
        .iter()
        .any(|p| key.starts_with(p))
        .then_some(SectionKind::AlsoBy)
}

/// "Chapter 3", "Kapitel drei: Title", "Part One", "Erstes Kapitel",
/// "Zweiter Teil – Title".
fn labelled(t: &str) -> Option<(SectionKind, u32, Option<String>)> {
    let mut words = t.splitn(3, char::is_whitespace);
    let first = words.next()?.to_lowercase();
    let second = words.next()?;
    let rest = words.next();

    let kind_of = |w: &str| {
        if vocab::CHAPTER_LABELS.contains(&w) {
            Some(SectionKind::Chapter)
        } else if vocab::PART_LABELS.contains(&w) {
            Some(SectionKind::Part)
        } else {
            None
        }
    };

    // Label first: "Chapter 3", "Teil eins".
    if let Some(kind) = kind_of(&first) {
        let (num_word, trailing) = split_number_token(second);
        if let Some(n) = parse_number(num_word) {
            let rest = join_rest(trailing, rest);
            return Some((kind, n, rest));
        }
    }
    // Ordinal first: "Erstes Kapitel", "Zweiter Teil".
    if let Some(n) = german_ordinal(&first) {
        let (label, trailing) = split_number_token(second);
        if let Some(kind) = kind_of(&label.to_lowercase()) {
            return Some((kind, n, join_rest(trailing, rest)));
        }
    }
    None
}

/// "3", "3.", "IV", "3. Title", "IV. Title", "3 – Title".
fn numbered(t: &str) -> Option<(u32, Option<String>)> {
    let (head, tail) = match t.split_once(char::is_whitespace) {
        Some((h, r)) => (h, Some(r)),
        None => (t, None),
    };
    let had_dot = head.ends_with(['.', ':', ')']);
    let token = head.trim_end_matches(['.', ':', ')']);
    let n = if token.chars().all(|c| c.is_ascii_digit()) && (1..=3).contains(&token.len()) {
        token.parse().ok()?
    } else {
        roman(token)?
    };
    match tail.map(str::trim) {
        None | Some("") => Some((n, None)),
        Some(rest) => {
            // A title after the number needs a separator ("3. Title",
            // "3 – Title"); "12 Monkeys" is a title, not chapter 12.
            let after_dash = rest.strip_prefix(['–', '—', '-']).map(str::trim);
            if had_dot || after_dash.is_some() {
                let r = after_dash.unwrap_or(rest);
                Some((n, (!r.is_empty()).then(|| r.to_string())))
            } else {
                None
            }
        }
    }
}

/// Split "3:" into ("3", ":") so a separator glued to the number is seen.
fn split_number_token(s: &str) -> (&str, &str) {
    let end = s
        .char_indices()
        .find(|(_, c)| matches!(c, ':' | '.' | ',' | '–' | '—'))
        .map_or(s.len(), |(i, _)| i);
    (&s[..end], &s[end..])
}

/// Title text after "Chapter 3": strip separators like ":", "–", ".".
fn join_rest(trailing: &str, rest: Option<&str>) -> Option<String> {
    let mut s = trailing.to_string();
    if let Some(r) = rest {
        s.push(' ');
        s.push_str(r);
    }
    let s = s.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '.' | ',' | '–' | '—' | '-')
    });
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

pub(crate) fn parse_number(word: &str) -> Option<u32> {
    let w = word.to_lowercase();
    if w.chars().all(|c| c.is_ascii_digit()) && (1..=3).contains(&w.len()) {
        return w.parse().ok();
    }
    roman(word)
        .or_else(|| english_number(&w))
        .or_else(|| german_number(&w))
}

/// Upper- or lowercase Roman numeral, 1..=399, strictly formed.
pub(crate) fn roman(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 9 {
        return None;
    }
    let upper = s.to_ascii_uppercase();
    // Mixed case ("Iv") is a word, not a numeral.
    if s != upper && s != s.to_ascii_lowercase() {
        return None;
    }
    let value = |c| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        _ => None,
    };
    let digits: Vec<u32> = upper.chars().map(value).collect::<Option<_>>()?;
    let mut total = 0;
    for (i, &d) in digits.iter().enumerate() {
        if digits.get(i + 1).is_some_and(|&n| n > d) {
            total -= d as i32;
        } else {
            total += d as i32;
        }
    }
    let n = u32::try_from(total).ok().filter(|&n| n > 0)?;
    // Reject non-canonical forms like "IIII" or "VX" ("I" and "C" are
    // also words in some contexts, but alone on a heading line they are
    // numerals).
    (to_roman(n) == upper).then_some(n)
}

fn to_roman(mut n: u32) -> String {
    const TABLE: &[(u32, &str)] = &[
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for &(v, r) in TABLE {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn english_number(w: &str) -> Option<u32> {
    use vocab::{EN_ONES as ONES, EN_TENS as TENS};
    if let Some(n) = ONES.iter().position(|&o| o == w) {
        return (n > 0).then_some(n as u32);
    }
    if let Some(n) = TENS.iter().position(|&t| !t.is_empty() && t == w) {
        return Some(n as u32 * 10);
    }
    let (tens, ones) = w.split_once('-')?;
    let t = TENS.iter().position(|&x| !x.is_empty() && x == tens)?;
    let o = ONES
        .iter()
        .position(|&x| x == ones)
        .filter(|&o| (1..10).contains(&o))?;
    Some((t * 10 + o) as u32)
}

fn german_number(w: &str) -> Option<u32> {
    use vocab::{DE_ONES as ONES, DE_TENS as TENS};
    if let Some(n) = ONES.iter().position(|&o| o == w) {
        return (n > 0).then_some(n as u32);
    }
    if let Some(n) = TENS.iter().position(|&t| !t.is_empty() && t == w) {
        return Some(n as u32 * 10);
    }
    // "einundzwanzig": the ones word drops its final "s" in compounds.
    let (ones, tens) = w.split_once("und")?;
    let t = TENS.iter().position(|&x| !x.is_empty() && x == tens)?;
    let o = if ones == "ein" {
        1
    } else {
        ONES.iter()
            .position(|&x| x == ones)
            .filter(|&o| (2..10).contains(&o))?
    };
    Some((t * 10 + o) as u32)
}

/// "Erstes", "zweiter", "Dritte" → 1, 2, 3 (up to 20).
pub(crate) fn german_ordinal(w: &str) -> Option<u32> {
    let stem = w
        .strip_suffix("es")
        .or_else(|| w.strip_suffix("er"))
        .or_else(|| w.strip_suffix('e'))?;
    vocab::DE_ORDINAL_STEMS
        .iter()
        .position(|&s| s == stem)
        .map(|i| i as u32 + 1)
}

/// Whether a paragraph's text is only a scene-break ornament: `***`,
/// `* * *`, `#`, `~~~`, `⁂`, `❦`, `• • •`, `o O o`, …
pub(crate) fn is_scene_break(text: &str) -> bool {
    let compact: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() || compact.len() > 12 {
        return false;
    }
    let ornament = |c: char| {
        matches!(
            c,
            '*' | '#' | '~' | '•' | '·' | '⁂' | '§' | '+' | '=' | '_' | '°' | '※' | '⸙' | '∗'
                | '⋆' | '◊' | '-' | '–' | '—'
        ) || ('\u{2600}'..='\u{27BF}').contains(&c) // misc symbols, dingbats (❦ ✻ ✦ …)
            || ('\u{25A0}'..='\u{25FF}').contains(&c) // geometric shapes (◆ ◇ ●)
            || ('\u{2055}'..='\u{205E}').contains(&c) // ⁕ ⁖ ⁘ …
    };
    if compact.iter().all(|&c| ornament(c)) {
        // A lone hyphen or dash is more likely a stray keystroke than a
        // scene break; require something else or repetition.
        return !(compact.len() == 1 && matches!(compact[0], '-' | '–' | '—' | '_' | '='));
    }
    // "o O o", "x x x"
    let letters: String = compact.iter().collect::<String>().to_lowercase();
    !letters.is_empty()
        && letters.len() >= 3
        && (letters.chars().all(|c| c == 'o') || letters.chars().all(|c| c == 'x'))
        && text.split_whitespace().count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(kind: SectionKind, number: Option<u32>, title: Option<&str>) -> HeadingInfo {
        HeadingInfo {
            kind,
            number,
            title: title.map(String::from),
        }
    }

    #[test]
    fn chapter_labels() {
        use SectionKind::*;
        let cases: &[(&str, HeadingInfo)] = &[
            ("Chapter 3", info(Chapter, Some(3), None)),
            ("CHAPTER THREE", info(Chapter, Some(3), None)),
            ("Chapter Twenty-One", info(Chapter, Some(21), None)),
            (
                "Chapter 3: The Visit",
                info(Chapter, Some(3), Some("The Visit")),
            ),
            (
                "Chapter 3 – The Visit",
                info(Chapter, Some(3), Some("The Visit")),
            ),
            ("Kapitel 12", info(Chapter, Some(12), None)),
            ("Kapi\u{ad}tel 12", info(Chapter, Some(12), None)),
            (
                "Kapitel 2: Eine kaiser\u{ad}liche Botschaft",
                info(Chapter, Some(2), Some("Eine kaiserliche Botschaft")),
            ),
            ("Kapitel drei", info(Chapter, Some(3), None)),
            ("Kapitel einundzwanzig", info(Chapter, Some(21), None)),
            (
                "Kapitel IV. Vor dem Gesetz",
                info(Chapter, Some(4), Some("Vor dem Gesetz")),
            ),
            ("Erstes Kapitel", info(Chapter, Some(1), None)),
            (
                "Zweites Kapitel: Auf der Galerie",
                info(Chapter, Some(2), Some("Auf der Galerie")),
            ),
            ("Part One", info(Part, Some(1), None)),
            (
                "Teil 2 – Vor dem Gesetz",
                info(Part, Some(2), Some("Vor dem Gesetz")),
            ),
            ("Zweiter Teil", info(Part, Some(2), None)),
            ("3", info(Chapter, Some(3), None)),
            ("3.", info(Chapter, Some(3), None)),
            ("XIV", info(Chapter, Some(14), None)),
            ("iv.", info(Chapter, Some(4), None)),
            (
                "3. Auf der Galerie",
                info(Chapter, Some(3), Some("Auf der Galerie")),
            ),
            ("IV. The Visit", info(Chapter, Some(4), Some("The Visit"))),
            (
                "3 – Auf der Galerie",
                info(Chapter, Some(3), Some("Auf der Galerie")),
            ),
        ];
        for (text, want) in cases {
            assert_eq!(&classify(text), want, "{text}");
        }
    }

    #[test]
    fn plain_titles_stay_titles() {
        for t in [
            "The Visit",
            "12 Monkeys",
            "1984",
            "Chapter and Verse",
            "I Am Legend",
            "Mix",
            "Civil",
            "Iv",
            "Partly Cloudy",
            "Teilchen",
        ] {
            let i = classify(t);
            assert_eq!(i, info(SectionKind::Chapter, None, Some(t)), "{t}");
            assert!(!classify(t).is_label(), "{t}");
        }
    }

    #[test]
    fn named_sections() {
        use SectionKind::*;
        for (t, k) in [
            ("Prolog", Prologue),
            ("PROLOGUE", Prologue),
            ("Epilog", Epilogue),
            ("Danksagung", Acknowledgments),
            ("Acknowledgements", Acknowledgments),
            ("Über die Autorin", AboutTheAuthor),
            ("About the Author", AboutTheAuthor),
            ("Nachwort", Afterword),
            ("Also by Jane Doe", AlsoBy),
            ("Weitere Bücher von Franz Kafka", AlsoBy),
            ("Inhalt", TableOfContents),
        ] {
            let i = classify(t);
            assert_eq!(i.kind, k, "{t}");
            assert_eq!(i.title.as_deref(), Some(t));
            assert_eq!(i.numbering(), Numbering::None);
            assert!(classify(t).is_label());
        }
    }

    #[test]
    fn numbering_from_info() {
        assert_eq!(classify("Chapter 3").numbering(), Numbering::Explicit(3));
        assert_eq!(classify("The Visit").numbering(), Numbering::Auto);
        assert_eq!(classify("Part 2").numbering(), Numbering::Explicit(2));
        assert_eq!(classify("Zweiter Teil").numbering(), Numbering::Explicit(2));
        assert!(classify("Chapter 3").is_bare_label());
        assert!(!classify("Chapter 3: X").is_bare_label());
    }

    #[test]
    fn roman_numerals_are_canonical() {
        assert_eq!(roman("XLII"), Some(42));
        assert_eq!(roman("ix"), Some(9));
        assert_eq!(roman("IIII"), None);
        assert_eq!(roman("VX"), None);
        assert_eq!(roman("Ix"), None);
        assert_eq!(roman(""), None);
    }

    #[test]
    fn scene_breaks() {
        for t in [
            "***",
            "* * *",
            "#",
            "~~~",
            "⁂",
            "❦",
            "• • •",
            "o O o",
            "x x x",
            "---",
            "◆",
        ] {
            assert!(is_scene_break(t), "{t}");
        }
        for t in ["", "-", "Ox", "Hello", "* not a break", "xox", "1"] {
            assert!(!is_scene_break(t), "{t}");
        }
    }
}
