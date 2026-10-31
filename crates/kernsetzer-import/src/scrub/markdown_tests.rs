use super::*;

fn scrub_text(src: &str) -> String {
    scrub(src.as_bytes(), |_| None).unwrap().text
}

#[test]
fn syntax_stays_and_prose_does_not() {
    // Lines of the repository's fixtures (links.md, footnotes.md, verse.md,
    // images.md).
    let src = "Title: Links\nAuthor: Jane Austen\nLang: en\n\n\
               # About the Author {-}\n\n\
               However little known the feelings or views of such a man may be on his first entering a neighbourhood. Her website is <https://example.com/austen>, \
               and she answers letters sent to <mailto:jane@example.com>.\n\n\
               She was a woman of [mean *understanding*](https://example.com/austen/letters \
               \"Mr. Bennet made no answer\") and keeps [a list of daughters](<https://example.com/lights (all)>).[^w]\n\n\
               [^w]: The list is at [the same family](https://example.com/austen/lights).\n\n\
               > You want to tell me, and I have no objection to hearing it[^log].\n\n\
               She was a woman of mean understanding, little information, and uncertain [temper]{.smallcaps}.\n\n\
               :::verse\nNun bin ich aber Arzt,\\\n:::\n\n\\newpage\n\n\
               ![Das nächste *Dorf*](img/dorf.png \"Ein Landarzt, Kurt Wolff 1919\"){width=60%}\n\n***\n";
    let out = scrub_text(src);
    let (a, b): (Vec<&str>, Vec<&str>) = (src.lines().collect(), out.lines().collect());
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.chars().count(), y.chars().count(), "{x}");
    }
    for kept in [
        "Title: ",
        "Author: ",
        "Lang: en",
        "# ",
        " {-}",
        "<https://example.com/austen>",
        "<mailto:jane@example.com>",
        "(https://example.com/austen/letters ",
        "(<https://example.com/lights (all)>)",
        "[^w]",
        "[^w]: ",
        "(https://example.com/austen/lights)",
        "> ",
        "[^log]",
        "]{.smallcaps}",
        ":::verse",
        "\n:::\n",
        "\\newpage",
        "![",
        "](img/dorf.png ",
        "){width=60%}",
        "***",
    ] {
        assert!(out.contains(kept), "{kept:?} missing from:\n{out}");
    }
    for gone in [
        "Austen",
        "neighbourhood",
        "understanding",
        "made no answer",
        "daughters",
        "same family",
        "objection",
        "temper",
        "Arzt",
        "Dorf",
        "Landarzt",
    ] {
        assert!(!out.contains(gone), "{gone:?} left in:\n{out}");
    }
}

#[test]
fn yaml_front_matter_keeps_keys_and_language() {
    // yaml-front-matter.md
    let src = "---\ntitle: \"Pride and Prejudice: A Novel\"\nauthor:\n  - Jane Austen\n\
               lang: en-GB # British spelling throughout\n\
               rights: © 2026 Jane Austen. All rights reserved.\n\
               # A YAML comment line is skipped.\n---\n\n# Chapter I\n\nMr. Bennet made no answer.\n";
    let out = scrub_text(src);
    for kept in [
        "---\ntitle: \"",
        "\nauthor:\n  - ",
        "\nlang: en-GB # ",
        "\nrights: © 2026 ",
        "All rights reserved.",
        "# ",
    ] {
        assert!(out.contains(kept), "{kept:?}\n{out}");
    }
    for gone in ["Prejudice", "Austen", "YAML comment", "answer", "British"] {
        assert!(!out.contains(gone), "{gone:?}\n{out}");
    }
}

#[test]
fn bom_line_endings_and_a_missing_final_newline_stay() {
    let src =
        "\u{feff}# Chapter I\r\n\r\nIt is a truth universally acknowledged, that a single man";
    let out = scrub_text(src);
    assert!(out.starts_with("\u{feff}# Chapter I\r\n\r\n"));
    assert_eq!(out.len(), src.len());
    assert!(!out.contains("truth"));
}

#[test]
fn a_dedication_keeps_its_first_words() {
    // dedication.md
    let out =
        scrub_text("# Chapter I\n\n*For my dear Mr. Bennet, said his lady to him one day,*\n");
    assert!(out.contains("\n*For "), "{out}");
    assert!(!out.contains("Bennet"), "{out}");
}

#[test]
fn invalid_utf8_is_an_error() {
    assert!(matches!(
        scrub(&[0xff, 0xfe, 0x80], |_| None),
        Err(ScrubError::NotUtf8)
    ));
}

#[test]
fn pictures_the_importer_asks_for_come_back_blank() {
    let png = setzer_model::asset::testdata::png_3x2_300dpi();
    let src = "# Eins\n\n![Das nächste *Dorf*](img/dorf.png \"Ein Landarzt\")\n\n![Fehlt](img/fehlt.png)\n";
    let got = scrub(src.as_bytes(), |p| {
        (p == "img/dorf.png").then(|| png.clone())
    })
    .unwrap();
    assert_eq!(got.images.len(), 1);
    assert_eq!(got.images[0].0, "img/dorf.png");
    let a = setzer_model::asset::Asset::new("a.png", got.images[0].1.clone());
    assert_eq!((a.width_px, a.height_px), (Some(3), Some(2)));
    assert!((a.dpi.unwrap() - 300.0).abs() < 0.1);
    assert!(got.text.contains("](img/dorf.png ") && got.text.contains("](img/fehlt.png)"));
}

#[test]
fn a_comment_on_the_last_line_without_a_line_end_is_scrambled() {
    // A comment alone on the last line leaves nothing there once stripped.
    let src = "Title: Pride and Prejudice\n\n# Chapter 1\n\nIt is a truth universally acknowledged.\n<!-- However little known -->";
    let out = scrub_text(src);
    assert_eq!(out.len(), src.len());
    assert!(out.contains("\n<!-- ") && out.ends_with(" -->"), "{out}");
    assert!(!out.contains("little") && !out.contains("truth"), "{out}");
    // Several lines, and a blank line before.
    let src = "# Chapter 1\n\nIt is a truth.\n\n<!-- However\nlittle known -->";
    assert!(!scrub_text(src).contains("little"));
}

#[test]
fn a_line_right_after_a_heading_starts_a_paragraph() {
    let out = scrub_text("# Prolog\nFür Dr. Bucephalus\n");
    assert!(out.contains("\nFür "), "{out}");
    let out = scrub_text("Title: Der neue Advokat\nLang: de\nFür Dr. Bucephalus\n");
    assert!(out.contains("\nFür "), "{out}");
}

#[test]
fn a_hard_wrapped_paragraph_is_judged_whole() {
    // Austen, folded: the second source line alone is short, but the
    // paragraph is not, and "all about" is prose in it.
    let src = "# Chapter 1\n\n\"But it is,\" returned she; \"for Mrs. Long has just been here, and she\ntold me all about it.\"\n";
    let out = scrub_text(src);
    let second = out.lines().nth(3).unwrap();
    let words: Vec<&str> = second.split(' ').collect();
    assert!(words[2] != "all" && words[3] != "about", "{second}");
    // A line of its own is a mark: "END" stays.
    let out = scrub_text("# Chapter 1\n\nIt is a truth.\n\nEND\n");
    assert!(out.ends_with("\nEND\n"), "{out}");
}

#[test]
fn a_declared_language_needs_no_function_words() {
    let text = "However little known the feelings or views of such a man may be on his first entering a neighbourhood, this truth is so well fixed in the minds of the surrounding families.";
    let declared = scrub_text(&format!("Lang: en\n\n{text}\n"));
    assert!(
        !declared.contains(" the ") && !declared.contains(" of "),
        "{declared}"
    );
    let guessed = scrub_text(&format!("{text}\n"));
    assert!(
        guessed.contains(" the ") && guessed.contains(" of "),
        "{guessed}"
    );
}

#[test]
fn a_table_caption_keeps_its_marker() {
    // tables-captions.md: the importer reads the `Table:` of a caption line,
    // and the delimiter row is syntax.
    let src = "# Chapter\n\nTable: A single man of large fortune\n\n\
               | Gentleman | A year |\n|:----------|-------:|\n| Mr. Bingley | four thousand |\n\n\
               Table:They found Mr. Bennet still up\n";
    let out = scrub_text(src);
    assert!(out.contains("\nTable: "), "{out}");
    assert!(out.contains("|:----------|-------:|"), "{out}");
    assert!(!out.contains("fortune"), "{out}");
    assert!(!out.contains("Gentleman"), "{out}");
    // No space after the colon: not a caption, so an ordinary word.
    assert!(!out.contains("Table:They"), "{out}");
}
