//! Replacing letters: the part of the scrambler that knows nothing about
//! files. It takes a text (as chars) and says which chars stay.
//!
//! What changes is only letters, and every letter changes (except ß, which
//! has no other member of its kind): a vowel becomes another vowel, an
//! umlaut another umlaut, a consonant another consonant, upper case stays
//! upper case. What stays: every non-letter; the words the importer reads,
//! each only in the place where it reads them (`vocab`, plus the headings'
//! numbers); and what the caller protects.
//!
//! A word is replaced the same way wherever it stands in one file, so the
//! equalities the importer looks at survive: a title page's author line and
//! `dc:creator`, a contents entry and its heading, "NORTHLINE" and
//! "Northline". Different files get different replacements: the seed is
//! taken from the whole file.

use std::cell::Cell;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;

use crate::{heading, lang, vocab};

/// A paragraph of at most this many words can be a title-page line, a
/// closing mark or a dateline; only there are dates, clock times and word
/// counts kept. Longer ones are prose.
const SHORT_LINE_WORDS: usize = 16;
/// A heading's label (a section name, "Chapter", a number word) opens its
/// line, however long the title: only among the first words of a line is it
/// kept. A number in front ("Teil 2", "Erstes Kapitel") narrows that to two.
const LABEL_WORDS: usize = 4;
const NUMBERED_LABEL_WORDS: usize = 2;
/// How many words from the start of the body keep the function words that
/// the importer counts to tell English from German: the importer looks at
/// its first `lang::SAMPLE_WORDS` words and no further, with a margin for
/// the words this module counts differently. After that they are prose.
pub(super) const LANG_BUDGET: usize = lang::SAMPLE_WORDS + lang::SAMPLE_WORDS / 5;

/// Where a text stands in its paragraph, for the places where the importer
/// reads words.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Context {
    /// The first line with letters begins a paragraph (a dedication or an
    /// author line is recognized there).
    pub para_start: bool,
    /// The text continues a paragraph that began on an earlier source line
    /// (hard-wrapped Markdown): no heading label opens it.
    pub continuation: bool,
    /// How many words the whole paragraph has, when the text is only a
    /// line of it.
    pub words: Option<usize>,
}

pub(super) struct Scrambler {
    seed: u64,
    /// For how many more words the language function words stay.
    lang_budget: Cell<usize>,
}

/// Hash of the bytes of a file: the seed of its scrambled copy.
pub(super) fn seed_of(bytes: &[u8]) -> u64 {
    splitmix(fnv1a(bytes))
}

impl Scrambler {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            lang_budget: Cell::new(usize::MAX),
        }
    }

    /// Keep the language function words for the next `words` words only
    /// (see `LANG_BUDGET`); `0` for text the importer does not count.
    pub fn set_lang_budget(&self, words: usize) {
        self.lang_budget.set(words);
    }

    /// `text` scrambled, char for char. A `'\n'` in it ends a line (a line
    /// break inside a paragraph); `protect[i]` keeps `text[i]` as it is;
    /// `para_start` says the first line begins a paragraph (where a
    /// dedication or an author line is recognized).
    pub fn scramble(&self, text: &[char], protect: &[bool], para_start: bool) -> Vec<char> {
        let cx = Context {
            para_start,
            ..Context::default()
        };
        self.scramble_in(text, protect, cx)
    }

    /// As `scramble`, for a text that stands in a paragraph as `cx` says.
    pub fn scramble_in(&self, text: &[char], protect: &[bool], cx: Context) -> Vec<char> {
        debug_assert_eq!(text.len(), protect.len());
        let mut out = text.to_vec();
        let mut start = 0;
        let mut first = cx.para_start;
        loop {
            let end = text[start..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(text.len(), |p| start + p);
            let line = Context {
                para_start: first,
                ..cx
            };
            self.line(
                &text[start..end],
                &protect[start..end],
                line,
                &mut out[start..end],
            );
            // A page break, a tab or a picture can come before the text:
            // the paragraph starts at its first letter.
            first = first && !text[start..end].iter().any(|c| c.is_alphabetic());
            if end >= text.len() {
                break;
            }
            start = end + 1;
        }
        out
    }

    fn line(&self, line: &[char], protect: &[bool], cx: Context, out: &mut [char]) {
        if line.is_empty() {
            return;
        }
        // "* * *", "o O o": the importer reads the ornament, not words.
        if heading::is_scene_break(&line.iter().collect::<String>()) {
            return;
        }
        let para_start = cx.para_start;
        let words = find_words(line, protect);
        let short = cx.words.unwrap_or(words.len()) <= SHORT_LINE_WORDS;
        let low: Vec<char> = line.iter().map(|&c| lower1(c)).collect();
        let lead = line.iter().position(|c| c.is_alphabetic()).unwrap_or(0);
        let has_digit = line.iter().any(char::is_ascii_digit);

        // Chars that stay because of a phrase the importer reads.
        let mut keep = vec![false; line.len()];
        // A phrase counts as a whole: "words" in "Zeichen", not in
        // "Schlüsselwörtern".
        let mut mark = |markers: &mut dyn Iterator<Item = &&str>, only_at_lead: bool| {
            for m in markers {
                let len = m.chars().count();
                for at in find_all(&low, m) {
                    let before = at == 0 || !low[at - 1].is_alphabetic();
                    let after = !m.ends_with(char::is_alphabetic)
                        || low.get(at + len).is_none_or(|c| !c.is_alphabetic());
                    if before && after && (!only_at_lead || at == lead) {
                        keep[at..at + len].fill(true);
                    }
                }
            }
        };
        mark(&mut vocab::WEB_MARKERS.iter(), false);
        // "Copyright" counts at the start of a line only.
        mark(
            &mut vocab::COPYRIGHT_MARKERS
                .iter()
                .filter(|m| **m == vocab::COPYRIGHT_LEAD),
            true,
        );
        mark(
            &mut vocab::COPYRIGHT_MARKERS
                .iter()
                .filter(|m| **m != vocab::COPYRIGHT_LEAD),
            false,
        );
        if short && has_digit {
            // A word count, a time: "approx. 4,000 words", "11:42 p.m.".
            let clock = vocab::CLOCK_WORDS
                .iter()
                .chain(vocab::CLOCK_AFTER_HOURS)
                .filter(|m| m.contains('.'));
            mark(&mut vocab::COUNT_WORDS.iter().chain(clock), false);
        }
        let mut author_line = false;
        if para_start {
            let rest: String = low[lead..].iter().collect();
            let starts = vocab::DEDICATION_STARTS
                .iter()
                .chain(vocab::AUTHOR_PREFIXES)
                .find(|p| rest.starts_with(*p));
            if let Some(p) = starts {
                keep[lead..lead + p.chars().count()].fill(true);
                author_line = vocab::AUTHOR_PREFIXES.contains(p);
            }
        }

        // What the line is, as the importer would read it: without the
        // marks that are syntax, and without the quotes and dashes around
        // it.
        let plain: String = line
            .iter()
            .zip(protect)
            .filter(|&(c, &p)| !p && !matches!(c, '*' | '_' | '`'))
            .map(|(&c, _)| c)
            .collect();
        let plain = plain.trim_matches(|c: char| !c.is_alphanumeric());
        let plain_low = plain.to_lowercase();
        // A heading label: how many of the line's first words may be read.
        let label_words = if cx.continuation {
            None
        } else {
            let info = heading::classify(plain);
            info.is_label().then_some(if info.number.is_some() {
                NUMBERED_LABEL_WORDS
            } else {
                LABEL_WORDS
            })
        };
        let end_line = {
            let w: Vec<&str> = plain_low.split_whitespace().collect();
            vocab::END_MARKS.contains(&w.as_slice())
        };
        let creator_line = vocab::CREATOR_PLACEHOLDERS.contains(&plain_low.as_str());
        let dateline = short && has_digit;

        for (n, w) in words.into_iter().enumerate() {
            let budget = self.lang_budget.get();
            self.lang_budget.set(budget.saturating_sub(1));
            if w.clone().any(|i| keep[i]) {
                continue;
            }
            let letters: Vec<usize> = w.filter(|&i| line[i].is_alphabetic()).collect();
            let word: String = letters.iter().map(|&i| line[i]).collect();
            let key: String = word.chars().map(lower1).collect();
            let after_digit = line[..letters[0]]
                .iter()
                .rev()
                .find(|c| !c.is_whitespace())
                .is_some_and(char::is_ascii_digit);
            let read = (budget > 0 && language_word(&key))
                || (label_words.is_some_and(|m| n < m) && structure_word(&word))
                || ((end_line || creator_line) && structure_word(&word))
                // "15 November 1978", "23.15 Uhr", "8 h".
                || (dateline && vocab::MONTHS.contains(&key.as_str()))
                || (dateline
                    && after_digit
                    && (vocab::CLOCK_WORDS.contains(&key.as_str())
                        || vocab::CLOCK_AFTER_HOURS.contains(&key.as_str())))
                // "by Jane and Cassandra Austen", "von Franz Kafka".
                || (author_line && vocab::AUTHOR_PARTICLES.contains(&key.as_str()));
            if read {
                continue;
            }
            let new = self.replacement(&key);
            for (&i, &r) in letters.iter().zip(&new) {
                out[i] = with_case_of(line[i], r);
            }
        }
    }

    /// The lowercase letters that stand in for the lowercase word `key`.
    /// Never a word the importer would read, unless `key` is one itself.
    fn replacement(&self, key: &str) -> Vec<char> {
        let letters: Vec<char> = key.chars().collect();
        let mut attempt = 0u64;
        loop {
            let mut rng = Rng(splitmix(self.seed ^ fnv1a(key.as_bytes())) ^ attempt);
            let new: Vec<char> = letters
                .iter()
                .map(|&c| replace_letter(c, &mut rng))
                .collect();
            let text: String = new.iter().collect();
            if attempt >= 64 || !reserved(&text) || reserved(key) {
                return new;
            }
            attempt += 1;
        }
    }
}

/// Words of `line`: runs of letters, with the zero-width and combining
/// chars that sit inside a word, outside what is protected.
fn find_words(line: &[char], protect: &[bool]) -> Vec<Range<usize>> {
    let mut words = Vec::new();
    let mut i = 0;
    while i < line.len() {
        if protect[i] || !line[i].is_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < line.len() && !protect[i] && (line[i].is_alphabetic() || is_joiner(line[i])) {
            i += 1;
        }
        words.push(start..i);
    }
    words
}

/// Soft hyphen, zero-width chars and combining marks: part of the word
/// they are in, never changed.
fn is_joiner(c: char) -> bool {
    matches!(c,
        '\u{ad}' | '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'
        | '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}' | '\u{fe00}'..='\u{fe0f}' | '\u{fe20}'..='\u{fe2f}')
}

fn find_all(hay: &[char], needle: &str) -> Vec<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || hay.len() < n.len() {
        return Vec::new();
    }
    (0..=hay.len() - n.len())
        .filter(|&i| hay[i..i + n.len()] == n[..])
        .collect()
}

/// One lowercase char, or the char itself when lowercasing changes the
/// length (so scrambling stays char for char).
fn lower1(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// `r` in the case of `original`.
fn with_case_of(original: char, r: char) -> char {
    if !original.is_uppercase() {
        return r;
    }
    if r == 'ß' {
        return 'ẞ';
    }
    let mut up = r.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => r,
    }
}

// ------------------------------------------------------- what is read

fn language_word(key: &str) -> bool {
    lang::EN_WORDS.contains(&key) || lang::DE_WORDS.contains(&key)
}

/// A word the structure pass or the heading rules read, in `word`'s case
/// ("IV" is a numeral, "Iv" is not): section names, labels, number words,
/// ordinals, end marks, months, particles of names.
fn structure_word(word: &str) -> bool {
    let key: String = word.chars().map(lower1).collect();
    structure_words().contains(key.as_str())
        || heading::parse_number(word).is_some()
        || heading::german_ordinal(&key).is_some()
}

/// Any word that means something to the importer, in any context, for
/// telling a replacement from a word it would read.
fn reserved(key: &str) -> bool {
    language_word(key) || structure_word(key) || prefix_words().contains(key)
}

/// The words of the importer's phrase lists (`vocab`), lowercase, two
/// letters or more (single letters of "a.m." are kept as phrases). Not the
/// starts of dedications and author lines: those count at the start of a
/// paragraph only (`prefix_words`).
fn structure_words() -> &'static HashSet<String> {
    static WORDS: OnceLock<HashSet<String>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut set = HashSet::new();
        for (_, names) in vocab::SECTION_NAMES {
            add_words(&mut set, names);
        }
        let lists: &[&[&str]] = &[
            vocab::ALSO_BY,
            vocab::CHAPTER_LABELS,
            vocab::PART_LABELS,
            vocab::EN_ONES,
            vocab::EN_TENS,
            vocab::DE_ONES,
            vocab::DE_TENS,
            vocab::AUTHOR_PARTICLES,
            vocab::MONTHS,
            vocab::CLOCK_WORDS,
            vocab::COPYRIGHT_MARKERS,
            vocab::WEB_MARKERS,
            vocab::COUNT_WORDS,
            vocab::CREATOR_PLACEHOLDERS,
        ];
        for list in lists {
            add_words(&mut set, list);
        }
        for mark in vocab::END_MARKS {
            add_words(&mut set, mark);
        }
        set
    })
}

/// The words of the starts of dedications and author lines.
fn prefix_words() -> &'static HashSet<String> {
    static WORDS: OnceLock<HashSet<String>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut set = HashSet::new();
        add_words(&mut set, vocab::DEDICATION_STARTS);
        add_words(&mut set, vocab::AUTHOR_PREFIXES);
        set
    })
}

fn add_words(set: &mut HashSet<String>, phrases: &[&str]) {
    for phrase in phrases {
        for w in phrase.split(|c: char| !c.is_alphabetic()) {
            if w.chars().count() >= 2 {
                set.insert(w.to_string());
            }
        }
    }
}

// ------------------------------------------------------ letter kinds

const VOWELS: &str = "aeiouàáâãåāăąæèéêēėęìíîīįòóôõøōőùúûūůűųœ";
const UMLAUTS: [char; 3] = ['ä', 'ö', 'ü'];
const PLAIN_VOWELS: [char; 5] = ['a', 'e', 'i', 'o', 'u'];
const CONSONANTS: [char; 20] = [
    'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'n', 'p', 'q', 'r', 's', 't', 'v', 'w', 'x',
    'z',
];
const CYRILLIC_VOWELS: &str = "аеёиоуыэюяіїєѣ";
const CYRILLIC_VOWEL_SET: [char; 9] = ['а', 'е', 'и', 'о', 'у', 'ы', 'э', 'ю', 'я'];
const CYRILLIC_CONSONANTS: [char; 20] = [
    'б', 'в', 'г', 'д', 'ж', 'з', 'к', 'л', 'м', 'н', 'п', 'р', 'с', 'т', 'ф', 'х', 'ц', 'ч', 'ш',
    'щ',
];
const GREEK_VOWELS: &str = "αεηιουωάέήίόύώ";
const GREEK_VOWEL_SET: [char; 7] = ['α', 'ε', 'η', 'ι', 'ο', 'υ', 'ω'];
const GREEK_CONSONANTS: [char; 14] = [
    'β', 'γ', 'δ', 'ζ', 'θ', 'κ', 'λ', 'μ', 'ν', 'ξ', 'π', 'ρ', 'σ', 'τ',
];

/// A lowercase letter replaced by another of its kind, never by itself
/// (unless it is alone in its kind: ß; or in a script block of one).
fn replace_letter(c: char, rng: &mut Rng) -> char {
    match c {
        'ß' => 'ß',
        'ä' | 'ö' | 'ü' | 'ë' | 'ï' | 'ÿ' => rng.pick_not(&UMLAUTS, c),
        _ if VOWELS.contains(c) => rng.pick_not(&PLAIN_VOWELS, c),
        '\u{400}'..='\u{52f}' => {
            if CYRILLIC_VOWELS.contains(c) {
                rng.pick_not(&CYRILLIC_VOWEL_SET, c)
            } else {
                rng.pick_not(&CYRILLIC_CONSONANTS, c)
            }
        }
        '\u{370}'..='\u{3ff}' | '\u{1f00}'..='\u{1fff}' => {
            if GREEK_VOWELS.contains(c) {
                rng.pick_not(&GREEK_VOWEL_SET, c)
            } else {
                rng.pick_not(&GREEK_CONSONANTS, c)
            }
        }
        '\u{3041}'..='\u{3096}' => rng.in_range_not('\u{3041}', '\u{3096}', c),
        '\u{30a1}'..='\u{30fa}' => rng.in_range_not('\u{30a1}', '\u{30fa}', c),
        '\u{4e00}'..='\u{9fff}' => rng.in_range_not('\u{4e00}', '\u{9fff}', c),
        '\u{ac00}'..='\u{d7a3}' => rng.in_range_not('\u{ac00}', '\u{d7a3}', c),
        _ => rng.pick_not(&CONSONANTS, c),
    }
}

// ---------------------------------------------------------- randomness

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        splitmix(self.0)
    }

    fn pick_not<const N: usize>(&mut self, set: &[char; N], not: char) -> char {
        let others = set.iter().filter(|&&c| c != not).count();
        let mut n = (self.next() % others.max(1) as u64) as usize;
        for &c in set {
            if c == not {
                continue;
            }
            if n == 0 {
                return c;
            }
            n -= 1;
        }
        not
    }

    fn in_range_not(&mut self, lo: char, hi: char, not: char) -> char {
        let span = hi as u32 - lo as u32; // `hi` is not `lo`: at least two chars
        let mut n = lo as u32 + (self.next() % u64::from(span)) as u32;
        if char::from_u32(n) == Some(not) {
            n = if n == hi as u32 { lo as u32 } else { n + 1 };
        }
        char::from_u32(n).unwrap_or(not)
    }
}

/// FNV-1a, 64 bit: a hash that is the same on every machine and release
/// (`DefaultHasher` is not).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn splitmix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
#[path = "words_tests.rs"]
mod tests;
