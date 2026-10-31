//! English or German from common function words, for manuscripts that do
//! not say (Markdown without `Lang:`) or may say wrong (.docx language
//! tags are often a template's default). Shared by both importers.

/// Words looked at to tell English from German.
pub(crate) const SAMPLE_WORDS: usize = 3000;

/// Words frequent in one language and rare in the other ("in", "was" and
/// "die" are left out on purpose).
pub(crate) const EN_WORDS: &[&str] = &[
    "the", "and", "of", "to", "he", "she", "it", "that", "you", "his", "her", "had", "with", "is",
    "not", "for", "but", "they", "on", "at",
];
pub(crate) const DE_WORDS: &[&str] = &[
    "der", "und", "das", "nicht", "sie", "er", "ich", "ist", "war", "zu", "den", "mit", "sich",
    "ein", "es", "auf", "dem", "aber", "eine", "auch",
];

/// `"en"` or `"de"` from the first words of `texts`, or `None` when there
/// are too few function words or neither language clearly wins.
pub(crate) fn detect<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let (mut en, mut de) = (0usize, 0usize);
    let words = texts
        .into_iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphabetic()))
        .filter(|w| !w.is_empty())
        .take(SAMPLE_WORDS);
    for w in words {
        let w = w.to_lowercase();
        en += usize::from(EN_WORDS.contains(&w.as_str()));
        de += usize::from(DE_WORDS.contains(&w.as_str()));
    }
    if en + de < 10 {
        None
    } else if en > 2 * de {
        Some("en")
    } else if de > 2 * en {
        Some("de")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::detect;

    #[test]
    fn english_german_or_unsure() {
        let en = "He had entertained hopes of being admitted to a sight of the young ladies, of whose beauty he had heard much; but he saw only the father.";
        let de = "Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor; ein Schwerkranker wartete auf mich in einem zehn Meilen entfernten Dorfe; starkes Schneegestöber füllte den weiten Raum zwischen mir und ihm; einen Wagen hatte ich, leicht, großräderig, ganz wie er für unsere Landstraßen taugt; in den Pelz gepackt, die Instrumententasche in der Hand, stand ich reisefertig schon auf dem Hofe.";
        assert_eq!(detect([en]), Some("en"));
        assert_eq!(detect([de]), Some("de"));
        assert_eq!(detect([en, de]), None);
        assert_eq!(detect(["Gut genug."]), None);
    }
}
