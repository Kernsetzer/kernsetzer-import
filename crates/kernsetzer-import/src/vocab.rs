//! The words the importers recognize, in one place.
//!
//! The heading rules, the structure pass, the dedication check and the
//! language count read their word lists from here, and the scrubber
//! (`scrub`) keeps exactly these words in a scrambled copy: a copy has to
//! import the way the original does, and the importer decides by these
//! words. Add a word here and both follow; a list typed anywhere else would
//! let the copy drift from the importer.

use setzer_model::SectionKind;

/// Section names ("Prolog", "About the Author"), matched against a whole
/// heading, lowercased, without a trailing `:` or `.`.
pub(crate) const SECTION_NAMES: &[(SectionKind, &[&str])] = &[
    (SectionKind::Prologue, &["prologue", "prolog"]),
    (SectionKind::Epilogue, &["epilogue", "epilog"]),
    (
        SectionKind::Foreword,
        &["foreword", "vorwort", "geleitwort"],
    ),
    (SectionKind::Preface, &["preface"]),
    (
        SectionKind::Afterword,
        &["afterword", "nachwort", "nachbemerkung"],
    ),
    (
        SectionKind::Acknowledgments,
        &["acknowledgments", "acknowledgements", "danksagung", "dank"],
    ),
    (
        SectionKind::AboutTheAuthor,
        &[
            "about the author",
            "über den autor",
            "über die autorin",
            "zum autor",
            "zur autorin",
            "die autorin",
            "der autor",
        ],
    ),
    (SectionKind::Dedication, &["dedication", "widmung"]),
    (
        SectionKind::TableOfContents,
        &[
            "contents",
            "table of contents",
            "inhalt",
            "inhaltsverzeichnis",
        ],
    ),
    (SectionKind::Copyright, &["copyright", "impressum"]),
];

/// Starts of an "also by" heading, lowercased.
pub(crate) const ALSO_BY: &[&str] = &[
    "also by ",
    "other books by ",
    "more books by ",
    "weitere bücher von ",
    "weitere romane von ",
    "ebenfalls von ",
    "mehr von ",
    "von derselben autorin",
    "vom selben autor",
    "von demselben autor",
];

/// Words that label a chapter or a part ("Chapter 3", "Teil 2").
pub(crate) const CHAPTER_LABELS: &[&str] = &["chapter", "kapitel", "chap.", "ch.", "kap."];
pub(crate) const PART_LABELS: &[&str] = &["part", "book", "teil", "buch"];

/// Number words, lowercased: index = value (the tens lists start at 20).
pub(crate) const EN_ONES: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
pub(crate) const EN_TENS: &[&str] = &[
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
pub(crate) const DE_ONES: &[&str] = &[
    "null",
    "eins",
    "zwei",
    "drei",
    "vier",
    "fünf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "elf",
    "zwölf",
    "dreizehn",
    "vierzehn",
    "fünfzehn",
    "sechzehn",
    "siebzehn",
    "achtzehn",
    "neunzehn",
];
pub(crate) const DE_TENS: &[&str] = &[
    "", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig",
];
/// Stems of the German ordinals ("erst" for "Erstes"), index + 1 = value.
pub(crate) const DE_ORDINAL_STEMS: &[&str] = &[
    "erst",
    "zweit",
    "dritt",
    "viert",
    "fünft",
    "sechst",
    "siebt",
    "acht",
    "neunt",
    "zehnt",
    "elft",
    "zwölft",
    "dreizehnt",
    "vierzehnt",
    "fünfzehnt",
    "sechzehnt",
    "siebzehnt",
    "achtzehnt",
    "neunzehnt",
    "zwanzigst",
];

/// A closing mark alone on the manuscript's last line, as lowercased words
/// ("The End", "— Ende —", "Fin.").
pub(crate) const END_MARKS: &[&[&str]] = &[
    &["end"],
    &["the", "end"],
    &["ende"],
    &["das", "ende"],
    &["fin"],
    &["finis"],
];

/// How a dedication starts, lowercased ("For …", "Für …").
pub(crate) const DEDICATION_STARTS: &[&str] = &[
    "für ",
    "fuer ",
    "for ",
    "to ",
    "in memory",
    "in memoriam",
    "in erinnerung",
    "in liebe",
    "gewidmet",
    "dedicated",
];

/// How an author line starts ("by Jane Doe", "von Lena Brandt"); compared
/// without regard to case.
pub(crate) const AUTHOR_PREFIXES: &[&str] = &["by ", "von "];
/// Lowercase words a name may contain without a capital.
pub(crate) const AUTHOR_PARTICLES: &[&str] = &[
    "and", "und", "&", "von", "van", "der", "den", "de", "du", "la", "le", "zu",
];

/// Month names (lowercased) a dateline may hold.
pub(crate) const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "januar",
    "jänner",
    "februar",
    "märz",
    "mai",
    "juni",
    "juli",
    "oktober",
    "dezember",
];
/// What follows an hour alone in a dateline ("11 p.m.", "8 Uhr").
pub(crate) const CLOCK_AFTER_HOURS: &[&str] = &["a.m.", "p.m.", "uhr"];
/// What follows hours and minutes ("11.42 p.m.", "23.15 Uhr", "23.15 h").
pub(crate) const CLOCK_WORDS: &[&str] = &["a.m.", "p.m.", "uhr", "h"];

/// The marker that counts at the start of a line only.
pub(crate) const COPYRIGHT_LEAD: &str = "copyright";
/// Marks of a copyright page, lowercased; "copyright" counts at the start
/// of a line, the rest anywhere in it (and "©" anywhere).
pub(crate) const COPYRIGHT_MARKERS: &[&str] = &[
    COPYRIGHT_LEAD,
    "isbn",
    "all rights reserved",
    "alle rechte vorbehalten",
];
/// Marks of a web address in a contact block.
pub(crate) const WEB_MARKERS: &[&str] = &["www.", "http://", "https://"];
/// Words of a word count in a contact block ("approx. 4,000 words").
pub(crate) const COUNT_WORDS: &[&str] = &["words", "wörter", "worte", "zeichen", "characters"];
/// `dc:creator` values producers fill in when they know no author.
pub(crate) const CREATOR_PLACEHOLDERS: &[&str] = &["unknown", "unbekannt", "author"];
