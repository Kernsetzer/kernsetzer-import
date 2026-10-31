use super::*;

fn scramble(text: &str) -> String {
    scramble_with(1, text, true)
}

fn scramble_with(seed: u64, text: &str, para_start: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let protect = vec![false; chars.len()];
    Scrambler::new(seed)
        .scramble(&chars, &protect, para_start)
        .into_iter()
        .collect()
}

/// A line long enough that nothing in it counts as a heading (Kafka,
/// "Ein Landarzt"; it holds "zehn", a number word).
const PROSE: &str = "Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor; \
                     ein Schwerkranker wartete auf mich in einem zehn Meilen entfernten Dorfe";
/// Austen, "Pride and Prejudice": enough English function words to tell
/// the language.
const AUSTEN: &str = "However little known the feelings or views of such a man may be on \
    his first entering a neighbourhood, this truth is so well fixed in the minds of the \
    surrounding families, that he is considered as the rightful property of some one or \
    other of their daughters.";

#[test]
fn letters_change_in_kind_and_everything_else_stays() {
    // Kafka and Austen, with a soft hyphen and a combining accent put in,
    // quote marks, digits and a dash.
    let src = "In seinem Äu\u{ad}ßern erinnert wenig an die Zeit, da er noch Streitroß Alexanders \
               von Mazedonien war. »Doch sah ich letzthin« e\u{301}ben 12, 3½ – \"My dear Mr. \
               Bennet,\" said his lady to him one day.";
    // No function words stay (the language count is off), so every letter
    // changes.
    let chars: Vec<char> = src.chars().collect();
    let sc = Scrambler::new(1);
    sc.set_lang_budget(0);
    let out: String = sc
        .scramble(&chars, &vec![false; chars.len()], true)
        .into_iter()
        .collect();
    assert_eq!(src.chars().count(), out.chars().count());
    for (a, b) in src.chars().zip(out.chars()) {
        if a.is_alphabetic() {
            assert!(b.is_alphabetic(), "{a} -> {b}");
            assert_eq!(a.is_uppercase(), b.is_uppercase(), "{a} -> {b}");
            if a != 'ß' {
                assert_ne!(a, b, "{src} -> {out}");
            }
            let kind = |c: char| {
                let l = lower1(c);
                if l == 'ß' {
                    2
                } else if "äöüëïÿ".contains(l) {
                    1
                } else if VOWELS.contains(l) {
                    0
                } else {
                    3
                }
            };
            assert_eq!(kind(a), kind(b), "{a} -> {b}");
        } else {
            assert_eq!(a, b);
        }
    }
    assert!(out.contains('ß'));
}

#[test]
fn it_is_deterministic_and_the_seed_matters() {
    assert_eq!(scramble(PROSE), scramble(PROSE));
    assert_ne!(scramble_with(1, PROSE, true), scramble_with(2, PROSE, true));
}

#[test]
fn a_word_is_replaced_alike_wherever_it_stands() {
    let out = scramble("Bucephalus Bucephalus, BUCEPHALUS bucephalus");
    let words: Vec<&str> = out.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    assert_eq!(words[0], words[1]);
    assert_eq!(words[0].to_lowercase(), words[2].to_lowercase());
    assert_eq!(words[0].to_lowercase(), words[3]);
    assert_ne!(words[0], "Bucephalus");
    assert!(words[2].chars().all(char::is_uppercase));
}

#[test]
fn the_importers_words_stay_in_short_lines_only() {
    for (text, kept) in [
        ("Chapter Three: The Storm", "Chapter Three: The "),
        ("Kapitel einundzwanzig", "Kapitel einundzwanzig"),
        ("Erstes Kapitel", "Erstes Kapitel"),
        ("Teil 2", "Teil 2"),
        ("IV. Der Sturm", "IV. Der "),
        ("Prolog", "Prolog"),
        ("About the Author", "About the Author"),
        ("Weitere Bücher von Franz Kafka", "Weitere Bücher von "),
        ("— Ende —", "— Ende —"),
        ("The End", "The End"),
        ("15 November 1978", "15 November 1978"),
        ("23.15 Uhr", "23.15 Uhr"),
        ("11:42 p.m.", "11:42 p.m."),
        ("Copyright 2020 Jane Austen", "Copyright 2020 "),
        ("ISBN 978-3-16-148410-0", "ISBN 978-3-16-148410-0"),
        ("www.example.org", "www."),
    ] {
        let out = scramble(text);
        assert!(out.starts_with(kept), "{text} -> {out}");
        assert_eq!(out == text, kept == text, "{text} -> {out}");
    }
    // A word count: the number and the unit stay, the words before do not.
    let out = scramble("Bucephalus, approx. 4,000 words");
    assert!(
        out.ends_with("4,000 words") && !out.starts_with("Bucephalus"),
        "{out}"
    );
    // In prose the same words are words.
    let out = scramble(PROSE);
    assert!(
        !out.contains("zehn") && !out.contains("Verlegenheit"),
        "{out}"
    );
    // A label opens a heading of any length, but does not stay in prose.
    assert!(scramble(&format!("Chapter 3: {PROSE}")).starts_with("Chapter 3: "));
    assert!(!scramble(&format!("{PROSE}. Chapter")).ends_with("Chapter"));
}

#[test]
fn a_name_in_a_chapter_title_does_not_stay() {
    let out = scramble("Chapter 3: Netherfield");
    assert!(out.starts_with("Chapter 3: "));
    assert!(!out.contains("Netherfield"));
}

#[test]
fn dedications_and_author_lines_keep_their_first_words() {
    assert!(scramble("For Mrs. Bennet").starts_with("For "));
    assert!(scramble("*Für Dr. Bucephalus*").starts_with("*Für "));
    assert!(scramble("by Jane Austen").starts_with("by "));
    assert!(scramble("Gewidmet Dr. Bucephalus").starts_with("Gewidmet "));
    // A long dedication too; but only at the start of a paragraph.
    let long = format!("Gewidmet {PROSE}");
    assert!(scramble_with(1, &long, true).starts_with("Gewidmet "));
    assert!(!scramble_with(1, &long, false).starts_with("Gewidmet"));
    assert!(!scramble("Chapter 3: Liebe").contains("Liebe"));
}

#[test]
fn language_words_stay_while_the_importer_counts() {
    let out = scramble(AUSTEN);
    assert_eq!(lang::detect([AUSTEN]), Some("en"));
    assert_eq!(lang::detect([out.as_str()]), Some("en"), "{out}");
    assert!(out.contains(" the ") && !out.contains("feelings"));

    // Past the words the importer counts they are prose like the rest.
    let chars: Vec<char> = AUSTEN.chars().collect();
    let protect = vec![false; chars.len()];
    let sc = Scrambler::new(1);
    sc.set_lang_budget(12);
    let out: String = sc.scramble(&chars, &protect, false).into_iter().collect();
    let (src, new): (Vec<&str>, Vec<&str>) =
        (AUSTEN.split(' ').collect(), out.split(' ').collect());
    assert_eq!((src[3], src[7]), (new[3], new[7])); // "the", "of": within 12 words
    assert_ne!(src[14], new[14]); // "his": after them
    assert_eq!(lang::detect([out.as_str()]), None);
    // And where the importer does not count at all, none stay.
    let sc = Scrambler::new(1);
    sc.set_lang_budget(0);
    let out: String = sc.scramble(&chars, &protect, false).into_iter().collect();
    assert!(!out.contains(" the ") && !out.contains(" of "), "{out}");
}

#[test]
fn heading_words_stay_where_the_importer_reads_them() {
    // Short, but not a heading, an end mark or a dateline: words, the
    // number words, "end" and "week" among them (Austen).
    for (src, changed) in [
        (
            "A single man of large fortune; four or five thousand a year.",
            &[1, 2, 4, 5, 6, 8, 9][..],
        ),
        (
            "Some of his servants are to be in the house by the end of next week.",
            &[3, 9, 12, 14, 15][..],
        ),
    ] {
        let out = scramble(src);
        let (a, b): (Vec<&str>, Vec<&str>) = (src.split(' ').collect(), out.split(' ').collect());
        for &i in changed {
            assert_ne!(a[i], b[i], "{src} -> {out}");
        }
    }
    // The same words in a line with a number are a dateline.
    assert!(scramble("March 3, 1812").starts_with("March "));
    // A word count's unit stays only in a short paragraph; in a long one
    // (hard-wrapped Markdown passes the paragraph's size) it is a word.
    let chars: Vec<char> = "told me all about it".chars().collect();
    let protect = vec![false; chars.len()];
    let long = Context {
        words: Some(20),
        continuation: true,
        ..Context::default()
    };
    let out: String = Scrambler::new(1)
        .scramble_in(&chars, &protect, long)
        .into_iter()
        .collect();
    let words: Vec<&str> = out.split(' ').collect();
    assert!(words[2] != "all" && words[3] != "about", "{out}");
}

#[test]
fn a_paragraph_starts_at_its_first_letter() {
    // A page break (a '\n' in the text), tab stops and a picture can come
    // before the dedication's first word.
    assert!(scramble("\nFür Dr. Bucephalus").starts_with("\nFür "));
    assert!(scramble("\n\n\nby Jane Austen").starts_with("\n\n\nby "));
    // Only the first line with letters: a later line is not a start.
    let out = scramble("Bucephalus\nFür Dr. Bucephalus");
    assert!(!out.contains("\nFür"), "{out}");
}

#[test]
fn ornaments_and_line_breaks() {
    assert_eq!(scramble("* * *"), "* * *");
    assert_eq!(scramble("o O o"), "o O o");
    assert_eq!(scramble("x x x"), "x x x");
    let out = scramble("Kapitel 1\nDer Anfang");
    assert!(out.starts_with("Kapitel 1\nDer "));
    assert_ne!(out, "Kapitel 1\nDer Anfang");
    assert_eq!(scramble(""), "");
}

#[test]
fn protected_chars_stay() {
    let chars: Vec<char> = "Bucephalus Alexanders".chars().collect();
    let mut protect = vec![false; chars.len()];
    protect[..10].fill(true);
    let out: String = Scrambler::new(3)
        .scramble(&chars, &protect, false)
        .into_iter()
        .collect();
    assert!(out.starts_with("Bucephalus "));
    assert!(!out.ends_with("Alexanders"));
}

#[test]
fn a_replacement_is_never_a_word_the_importer_reads() {
    for seed in 0..400u64 {
        let sc = Scrambler::new(seed);
        for key in [
            "tie", "vet", "one", "adn", "hr", "ix", "iv", "mai", "xi", "hes",
        ] {
            let new: String = sc.replacement(key).into_iter().collect();
            assert!(!reserved(&new) || reserved(key), "{key} -> {new}");
        }
    }
}

#[test]
fn other_scripts_and_odd_chars_do_not_panic() {
    for text in [
        "Привет, мир",
        "Καλημέρα κόσμε",
        "日本語のテキスト",
        "한국어 텍스트",
        "ǅ İ ı ŉ ẞ ﬃ",
        "naïve café",
        "\u{feff}\u{200b}",
    ] {
        let out = scramble(text);
        assert_eq!(out.chars().count(), text.chars().count(), "{text}");
    }
    assert_ne!(scramble("Привет"), "Привет");
}
