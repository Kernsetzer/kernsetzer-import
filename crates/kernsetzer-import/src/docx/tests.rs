//! Test helpers (synthetic .docx files) and end-to-end tests of `import()`.

use super::import;
use crate::{ImportError, Imported, WarningKind};
use setzer_model::{
    Block, ColumnAlign, Epigraph, Inline, Numbering, SectionKind, Table, TableCell, TableColumn,
};
use std::io::{Cursor, Write};

/// A zip archive with the given entries.
pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

pub(crate) fn empty_zip() -> Vec<u8> {
    zip_of(&[("[Content_Types].xml", b"<Types/>")])
}

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A synthetic .docx: body XML plus optional parts.
#[derive(Default)]
pub(crate) struct Docx {
    body: String,
    /// Extra `w:style` elements, e.g. from `style()`.
    styles: String,
    lang: Option<&'static str>,
    footnotes: Option<String>,
    numbering: Option<String>,
    core: Option<String>,
    images: Vec<(&'static str, Vec<u8>)>,
}

impl Docx {
    pub fn new(paras: &[String]) -> Self {
        Docx {
            body: paras.concat(),
            ..Docx::default()
        }
    }

    pub fn build(&self) -> Vec<u8> {
        let lang = self
            .lang
            .map(|l| {
                format!(
                    r#"<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="{l}"/></w:rPr></w:rPrDefault></w:docDefaults>"#
                )
            })
            .unwrap_or_default();
        let styles = format!(
            r#"<w:styles {NS}>{lang}<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>{}{}{}{}{}{}{}{}</w:styles>"#,
            style("Title", "Title", ""),
            style("Subtitle", "Subtitle", ""),
            style(
                "Heading1",
                "heading 1",
                r#"<w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr>"#
            ),
            style(
                "Heading2",
                "heading 2",
                r#"<w:pPr><w:outlineLvl w:val="1"/></w:pPr>"#
            ),
            style(
                "Heading3",
                "heading 3",
                r#"<w:pPr><w:outlineLvl w:val="2"/></w:pPr>"#
            ),
            style("Quote", "Quote", r#"<w:rPr><w:i/></w:rPr>"#),
            style("Verse", "Verse", "") + &style("Caption", "caption", ""),
            self.styles,
        );
        let document = format!(
            r#"<w:document {NS}><w:body>{}</w:body></w:document>"#,
            self.body
        );

        let mut rels =
            format!(r#"<Relationship Id="rStyles" Type="{REL}/styles" Target="styles.xml"/>"#);
        if self.footnotes.is_some() {
            rels += &format!(
                r#"<Relationship Id="rFoot" Type="{REL}/footnotes" Target="footnotes.xml"/>"#
            );
        }
        if self.numbering.is_some() {
            rels += &format!(
                r#"<Relationship Id="rNum" Type="{REL}/numbering" Target="numbering.xml"/>"#
            );
        }
        for (name, _) in &self.images {
            let id = name.split('.').next().unwrap_or_default();
            rels +=
                &format!(r#"<Relationship Id="{id}" Type="{REL}/image" Target="media/{name}"/>"#);
        }
        let doc_rels = format!(r#"<Relationships>{rels}</Relationships>"#);
        let mut root = format!(
            r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/>"#
        );
        if self.core.is_some() {
            root += r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>"#;
        }
        root += "</Relationships>";

        let media: Vec<(String, &[u8])> = self
            .images
            .iter()
            .map(|(n, b)| (format!("word/media/{n}"), b.as_slice()))
            .collect();
        let mut files: Vec<(&str, &[u8])> = vec![
            ("[Content_Types].xml", b"<Types/>"),
            ("_rels/.rels", root.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
        ];
        let footnotes = self
            .footnotes
            .as_ref()
            .map(|f| format!(r#"<w:footnotes {NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>{f}</w:footnotes>"#));
        if let Some(f) = &footnotes {
            files.push(("word/footnotes.xml", f.as_bytes()));
        }
        if let Some(n) = &self.numbering {
            files.push(("word/numbering.xml", n.as_bytes()));
        }
        if let Some(c) = &self.core {
            files.push(("docProps/core.xml", c.as_bytes()));
        }
        for (n, b) in &media {
            files.push((n, b));
        }
        zip_of(&files)
    }

    pub fn import(&self) -> Imported {
        import(&self.build()).unwrap()
    }
}

/// A paragraph style; `extra` holds its `w:basedOn`, `w:pPr` and `w:rPr`.
fn style(id: &str, name: &str, extra: &str) -> String {
    format!(
        r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/>{extra}</w:style>"#
    )
}

fn run(text: &str) -> String {
    format!(r#"<w:r><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

/// A body paragraph.
pub(crate) fn p(text: &str) -> String {
    format!("<w:p>{}</w:p>", run(text))
}

/// A paragraph in a style.
pub(crate) fn ps(style: &str, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
        run(text)
    )
}

/// A centered paragraph.
fn pc(text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr>{}</w:p>"#,
        run(text)
    )
}

/// An empty paragraph.
fn empty() -> String {
    "<w:p/>".into()
}

/// An empty paragraph holding a page break.
fn page_break() -> String {
    r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#.into()
}

fn core(title: &str, creator: &str, lang: &str) -> String {
    format!(
        r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>{creator}</dc:creator><dc:language>{lang}</dc:language></cp:coreProperties>"#
    )
}

fn para(text: &str) -> Block {
    Block::paragraph(vec![Inline::text(text)])
}

fn kinds(im: &Imported) -> Vec<WarningKind> {
    im.warnings.iter().map(|w| w.kind).collect()
}

fn outline(im: &Imported) -> Vec<(SectionKind, Option<&str>, Numbering)> {
    im.book
        .sections
        .iter()
        .map(|s| (s.kind, s.title.as_deref(), s.numbering.clone()))
        .collect()
}

#[test]
fn styled_german_novel() {
    let mut d = Docx::new(&[
        ps("Title", "Ein Landarzt"),
        ps("Subtitle", "Roman"),
        pc("von Franz Kafka"),
        page_break(),
        pc("Für Milena"),
        page_break(),
        ps("Heading1", "Prolog"),
        p("Aber gleich standen sie aufrecht, hochbeinig, mit dicht ausdampfendem Körper."),
        ps("Heading1", "Kapitel 1"),
        ps("Subtitle", "Auf der Galerie"),
        p("Das Dienstmädchen stand neben mir."),
        empty(),
        p("Nun bin ich aber Arzt."),
        pc("* * *"),
        format!(
            r#"<w:p>{}<w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
            run("Der neue Advokat ")
        ),
        ps("Heading1", "Kapitel 2: Sturm"),
        ps("Quote", "Noch standen treu die Pferde an ihren Plätzen."),
        ps("Verse", "Zeile eins"),
        ps("Verse", "Zeile zwei"),
        empty(),
        ps("Verse", "Zeile drei"),
        p("Ende."),
        ps("Heading1", "Danksagung"),
        p("Nun bin ich aber Arzt."),
    ]);
    d.lang = Some("de-AT");
    d.footnotes = Some(format!(
        r#"<w:footnote w:id="1">{}</w:footnote>"#,
        p("Dort sitzt er Tage und Jahre.")
    ));
    let im = d.import();

    let m = &im.book.metadata;
    assert_eq!(m.title, "Ein Landarzt");
    assert_eq!(m.subtitle.as_deref(), Some("Roman"));
    assert_eq!(m.authors, vec!["Franz Kafka"]);
    assert_eq!(m.lang.0, "de-AT");

    assert_eq!(
        outline(&im),
        vec![
            (SectionKind::Dedication, None, Numbering::None),
            (SectionKind::Prologue, Some("Prolog"), Numbering::None),
            (
                SectionKind::Chapter,
                Some("Auf der Galerie"),
                Numbering::Explicit(1)
            ),
            (SectionKind::Chapter, Some("Sturm"), Numbering::Explicit(2)),
            (
                SectionKind::Acknowledgments,
                Some("Danksagung"),
                Numbering::None
            ),
        ]
    );
    let s = &im.book.sections;
    assert_eq!(s[0].blocks, vec![para("Für Milena")]);
    assert_eq!(
        s[2].blocks,
        vec![
            para("Das Dienstmädchen stand neben mir."),
            Block::scene_break(),
            para("Nun bin ich aber Arzt."),
            Block::scene_break(),
            Block::paragraph(vec![
                Inline::text("Der neue Advokat"),
                Inline::Footnote {
                    blocks: vec![para("Dort sitzt er Tage und Jahre.")]
                },
            ]),
        ]
    );
    let line = |t: &str| vec![Inline::text(t)];
    assert_eq!(
        s[3].blocks,
        vec![
            Block::block_quote(vec![para("Noch standen treu die Pferde an ihren Plätzen.")]),
            Block::verse(vec![
                line("Zeile eins"),
                line("Zeile zwei"),
                vec![],
                line("Zeile drei")
            ]),
            para("Ende."),
        ]
    );

    assert_eq!(
        kinds(&im),
        vec![
            WarningKind::TitlePageReplaced,
            WarningKind::SceneBreaksFromBlankLines
        ]
    );
    let blank = &im.warnings[1];
    assert_eq!(blank.section, Some(2), "{blank:?}");
}

#[test]
fn unstyled_english_manuscript() {
    let mut d = Docx::new(&[
        pc("FIRST IMPRESSIONS"),
        pc("by Jane Doe"),
        page_break(),
        p("Chapter One"),
        p("A single man of large fortune; four or five thousand a year."),
        format!(
            "<w:p>{}<w:r><w:br/></w:r>{}</w:p>",
            run("Mr. Bennet made no answer."),
            run("Lizzy is not a bit better than the others.")
        ),
        page_break(),
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>The Storm</w:t></w:r></w:p>"#
            .into(),
        p("At such an assembly as this, it would be insupportable."),
        empty(),
        empty(),
        p("Her mind was less difficult to develope."),
        pc("#"),
        p("Bingley, his two sisters, the husband of the eldest, and another young man."),
    ]);
    // Word's default language says German, the text and dc:language
    // say British English.
    d.lang = Some("de-DE");
    d.core = Some(core("", "", "en-GB"));
    let im = d.import();

    let m = &im.book.metadata;
    assert_eq!(m.title, "FIRST IMPRESSIONS");
    assert_eq!(m.authors, vec!["Jane Doe"]);
    assert_eq!(m.lang.0, "en-GB");
    assert_eq!(
        outline(&im),
        vec![
            (SectionKind::Chapter, None, Numbering::Explicit(1)),
            (SectionKind::Chapter, Some("The Storm"), Numbering::Auto),
        ]
    );
    assert_eq!(
        im.book.sections[0].blocks[1],
        para("Mr. Bennet made no answer. Lizzy is not a bit better than the others.")
    );
    assert_eq!(
        im.book.sections[1].blocks,
        vec![
            para("At such an assembly as this, it would be insupportable."),
            Block::scene_break(),
            para("Her mind was less difficult to develope."),
            Block::scene_break(),
            para("Bingley, his two sisters, the husband of the eldest, and another young man."),
        ]
    );
    assert_eq!(
        kinds(&im),
        vec![
            WarningKind::TitlePageReplaced,
            WarningKind::LineBreaksJoined,
            WarningKind::SceneBreaksFromBlankLines,
        ]
    );
}

#[test]
fn book_title_heading_and_subheadings() {
    let im = Docx::new(&[
        ps("Heading1", "Mein Buch"),
        ps("Heading2", "Anfang"),
        p("Text."),
        ps("Heading2", "Mitte"),
        p("Mehr Text."),
        ps("Heading3", "Ein Zwischentitel"),
        p("Noch mehr."),
    ])
    .import();
    assert_eq!(im.book.metadata.title, "Mein Buch");
    assert_eq!(
        outline(&im),
        vec![
            (SectionKind::Chapter, Some("Anfang"), Numbering::Auto),
            (SectionKind::Chapter, Some("Mitte"), Numbering::Auto),
        ]
    );
    assert_eq!(
        im.book.sections[1].blocks[1],
        Block::heading(1, vec![Inline::text("Ein Zwischentitel")])
    );
}

#[test]
fn parts_and_chapters() {
    let im = Docx::new(&[
        ps("Heading1", "Prolog"),
        p("Vorher."),
        ps("Heading1", "Erster Teil"),
        ps("Heading2", "Erstes Kapitel"),
        p("Eins."),
        ps("Heading2", "Zweites Kapitel"),
        p("Zwei."),
        ps("Heading1", "Zweiter Teil"),
        ps("Heading2", "Drittes Kapitel"),
        p("Drei."),
        ps("Heading1", "Epilog"),
        p("Nachher."),
    ])
    .import();
    assert_eq!(
        outline(&im),
        vec![
            (SectionKind::Prologue, Some("Prolog"), Numbering::None),
            (SectionKind::Part, Some("Erster Teil"), Numbering::None),
            (SectionKind::Chapter, None, Numbering::Explicit(1)),
            (SectionKind::Chapter, None, Numbering::Explicit(2)),
            (SectionKind::Part, Some("Zweiter Teil"), Numbering::None),
            (SectionKind::Chapter, None, Numbering::Explicit(3)),
            (SectionKind::Epilogue, Some("Epilog"), Numbering::None),
        ]
    );
}

#[test]
fn front_matter_pages() {
    let im = Docx::new(&[
        pc("Ein Roman"),
        page_break(),
        p("© 2026 Franz Kafka"),
        p("Alle Rechte vorbehalten."),
        page_break(),
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>Das Meer vergisst nichts.</w:t></w:r></w:p>"#
            .into(),
        page_break(),
        p("Immer muß ich mich begnügen."),
        ps("Heading1", "Kapitel 1"),
        p("Text."),
    ])
    .import();
    let m = &im.book.metadata;
    assert_eq!(m.title, "Ein Roman");
    assert_eq!(m.copyright.as_deref(), Some("© 2026 Franz Kafka"));
    let kinds_: Vec<_> = im.book.sections.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds_,
        vec![
            SectionKind::Copyright,
            SectionKind::Epigraph,
            SectionKind::Other,
            SectionKind::Chapter
        ]
    );
    assert_eq!(
        kinds(&im),
        vec![
            WarningKind::TextBeforeFirstChapter,
            WarningKind::TitlePageReplaced
        ]
    );
    assert_eq!(im.warnings[0].section, Some(2));
}

#[test]
fn no_headings_is_one_chapter() {
    let im = Docx::new(&[p("Just text."), p("More text.")]).import();
    assert_eq!(
        outline(&im),
        vec![(SectionKind::Chapter, None, Numbering::Auto)]
    );
    assert_eq!(im.book.sections[0].blocks.len(), 2);
    assert_eq!(
        kinds(&im),
        vec![WarningKind::NoChapters, WarningKind::NoTitle]
    );
}

#[test]
fn toc_lists_images_tables() {
    let toc = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>TOC \o "1-3"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Chapter 1 ..... 3</w:t></w:r></w:p><w:p><w:r><w:t>Chapter 2 ..... 9</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let item = |t: &str| {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>{}</w:p>"#,
            run(t)
        )
    };
    let image = r#"<w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Picture 1" descr="A lighthouse"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="image1"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
    let mut d = Docx::new(&[
        toc.into(),
        page_break(),
        ps("Heading1", "Chapter 1"),
        p("Before."),
        item("a"),
        item("b"),
        image.into(),
        ps("Caption", "A single man of large fortune"),
        p("After."),
        page_break(),
        p("She has two nieces of her own."),
        ps("Heading1", "Chapter 2"),
        format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", p("cell")),
    ]);
    d.numbering = Some(format!(
        r#"<w:numbering {NS}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    ));
    d.images = vec![(
        "image1.png",
        setzer_model::asset::testdata::png_3x2_300dpi(),
    )];
    d.core = Some(core("Leuchtfeuer", "Jane Doe", ""));
    let im = d.import();

    assert_eq!(im.book.metadata.title, "Leuchtfeuer");
    assert_eq!(im.book.metadata.authors, vec!["Jane Doe"]);
    assert_eq!(im.assets.len(), 1);
    let asset = im.assets[0].id.clone();
    assert_eq!(
        im.book.sections[0].blocks,
        vec![
            para("Before."),
            Block::list(true, vec![vec![para("a")], vec![para("b")]]),
            Block::image(
                asset,
                "A lighthouse",
                Some(vec![Inline::text("A single man of large fortune")])
            ),
            para("After."),
            para("She has two nieces of her own."),
        ]
    );
    assert_eq!(im.book.sections[1].blocks, vec![para("cell")]);
    let w: Vec<_> = im.warnings.iter().map(|w| (w.kind, w.section)).collect();
    assert_eq!(
        w,
        vec![
            (WarningKind::TableFlattened, Some(1)),
            (WarningKind::TableOfContentsRemoved, None),
            (WarningKind::PageBreaksIgnored, Some(0)),
        ]
    );
}

/// `wp:inline` / `wp:anchor` XML for a picture of `inches` wide.
fn picture(inches: f64, kind: &str, wrap: &str) -> String {
    let cx = (inches * 914_400.0) as i64;
    let anchor = if kind == "anchor" {
        r#" distT="0" behindDoc="0""#
    } else {
        ""
    };
    format!(
        r#"<w:r><w:drawing><wp:{kind}{anchor}><wp:extent cx="{cx}" cy="{cx}"/>{wrap}<wp:docPr id="1" name="Picture 1"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="image1"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx="1" cy="1"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:{kind}></w:drawing></w:r>"#
    )
}

fn picture_doc(paras: &[String]) -> Docx {
    let mut d = Docx::new(paras);
    d.images = vec![(
        "image1.png",
        setzer_model::asset::testdata::png_3x2_300dpi(),
    )];
    d
}

fn images(im: &Imported) -> Vec<&Block> {
    im.book
        .sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter(|b| matches!(b, Block::Image { .. }))
        .collect()
}

#[test]
fn picture_width_without_section_properties_is_a_letter_page() {
    // Word's defaults: 8.5 x 11 in with one-inch margins, a 6.5 in text block.
    let im = picture_doc(&[
        p("Text."),
        format!("<w:p>{}</w:p>", picture(3.25, "inline", "")),
    ])
    .import();
    assert_eq!(
        images(&im),
        [&Block::image(im.assets[0].id.clone(), "", None).with_width(50)]
    );
}

#[test]
fn picture_width_uses_the_section_it_is_in() {
    // The first section's `sectPr` is in its last paragraph; the second
    // section's is the body's last child. 4.25 in is 65 percent of 6.5 in
    // and 100 percent of the second section's 4.25 in.
    let narrow = r#"<w:sectPr><w:pgSz w:w="8640" w:h="12960"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:gutter="0"/></w:sectPr>"#;
    let wide = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:pPr></w:p>"#;
    let pic = || format!("<w:p>{}</w:p>", picture(4.25, "inline", ""));
    let im = picture_doc(&[
        ps("Heading1", "One"),
        pic(),
        wide.into(),
        ps("Heading1", "Two"),
        pic(),
        narrow.into(),
    ])
    .import();
    let widths: Vec<_> = images(&im).iter().map(|b| b.image_width()).collect();
    assert_eq!(widths, [Some(65), Some(100)]);
}

#[test]
fn pictures_in_tables_and_notes_keep_no_size() {
    let cell = |c: &str| format!("<w:tc>{c}</w:tc>");
    let pic = format!("<w:p>{}</w:p>", picture(3.25, "inline", ""));
    let table = format!(
        "<w:tbl><w:tr>{}{}</w:tr></w:tbl>",
        cell(&pic),
        cell(&p("beside"))
    );
    let mut d = picture_doc(&[
        p("Text."),
        table,
        format!(
            "<w:p>{}{}</w:p>",
            run("Note"),
            r#"<w:r><w:footnoteReference w:id="1"/></w:r>"#
        ),
    ]);
    d.footnotes = Some(format!(
        r#"<w:footnote w:id="1"><w:p>{}</w:p></w:footnote>"#,
        picture(1.0, "anchor", "<wp:wrapNone/>")
    ));
    let im = d.import();
    for b in images(&im) {
        assert_eq!(b.image_width(), None);
    }
    assert!(
        im.warnings
            .iter()
            .all(|w| w.kind != WarningKind::ImagePositionDropped)
    );
}

#[test]
fn one_warning_for_pictures_over_the_text() {
    let over = |behind: &str| {
        format!(
            "<w:p>{}{}</w:p>",
            picture(1.0, "anchor", &format!("<wp:wrapNone/>{behind}")),
            run("Text.")
        )
    };
    let im = picture_doc(&[p("Text."), over(""), over(""), over("")]).import();
    let dropped: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::ImagePositionDropped)
        .collect();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].location, 2);
    assert!(dropped[0].message.contains("3 pictures"));
    assert_eq!(images(&im).len(), 3);
}

#[test]
fn not_a_manuscript() {
    assert!(matches!(import(b""), Err(ImportError::Empty)));
    assert!(matches!(
        import(b"plain text"),
        Err(ImportError::NotDocx(_))
    ));
    assert!(matches!(import(&empty_zip()), Err(ImportError::NotDocx(_))));
    let blank = Docx::new(&[empty(), page_break()]).build();
    assert!(matches!(import(&blank), Err(ImportError::Empty)));
}

#[test]
fn large_bold_lines_are_headings_without_page_breaks() {
    let sized = |t: &str, sz: u16, extra: &str, ppr: &str| {
        format!(
            r#"<w:p><w:pPr>{ppr}</w:pPr><w:r><w:rPr>{extra}<w:sz w:val="{sz}"/></w:rPr><w:t xml:space="preserve">{t}</w:t></w:r></w:p>"#
        )
    };
    let center = r#"<w:jc w:val="center"/>"#;
    let body = |t: &str| sized(t, 24, "", "");
    let im = Docx::new(&[
        sized("Glass", 48, "<w:b/>", center),
        sized("by Jane Doe", 24, "", center),
        sized("The Visit", 36, "<w:b/>", ""),
        body("The evening altogether passed off pleasantly to the whole family."),
        body("This was invitation enough."),
        sized("Spring", 36, "<w:b/>", ""),
        body("This was invitation enough."),
    ])
    .import();
    assert_eq!(im.book.metadata.title, "Glass");
    assert_eq!(
        outline(&im),
        vec![
            (SectionKind::Chapter, Some("The Visit"), Numbering::Auto),
            (SectionKind::Chapter, Some("Spring"), Numbering::Auto),
        ]
    );
}

#[test]
fn chapter_epigraphs() {
    let im = Docx::new(&[
        ps("Heading1", "Kapitel 1"),
        // The Quote style is italic: that italic is the epigraph's style.
        ps("Quote", "Armer Junge, dir ist nicht zu helfen."),
        ps("Quote", "– Aus einem Landarzt"),
        p("Ich schwang mich aufs Pferd."),
        ps("Heading1", "Kapitel 2"),
        // A letter: set off, but no attribution line.
        ps("Quote", "Liebe Milena,"),
        ps("Quote", "Ich schwang mich aufs Pferd."),
        p("Merke aber: Ich bin mächtig."),
    ])
    .import();
    let s = &im.book.sections;
    assert_eq!(
        s[0].epigraph,
        Some(Epigraph {
            text: vec![Inline::text("Armer Junge, dir ist nicht zu helfen.")],
            attribution: Some("Aus einem Landarzt".into()),
        })
    );
    assert_eq!(s[0].blocks, vec![para("Ich schwang mich aufs Pferd.")]);
    assert_eq!(s[1].epigraph, None);
    assert!(
        matches!(s[1].blocks[0], Block::BlockQuote { .. }),
        "{:?}",
        s[1]
    );
    assert_eq!(kinds(&im), vec![WarningKind::NoTitle]);
}

/// Plain text of every block in the book, for "this never prints" checks.
fn book_text(im: &Imported) -> String {
    serde_json::to_string(&im.book.sections).unwrap()
}

#[test]
fn manuscript_title_page_with_contact_block() {
    // Standard manuscript format: the author's contact block and a
    // right-tabbed word count at the top, the title and byline centered
    // halfway down. Then the same page with the contact block at the foot.
    let contact = || {
        vec![
            format!(
                "<w:p>{}<w:r><w:tab/></w:r>{}</w:p>",
                run("Charles Bingley"),
                run("about 3,500 words")
            ),
            p("42 Tamsin Lane"),
            p("Meryton, NH 03301"),
            p("(603) 555-0199"),
            p("wren.halloway@example.org"),
        ]
    };
    let title = || vec![pc("SENSE AND SENSIBILITY"), pc("by Charles Bingley")];
    let story = || {
        vec![
            page_break(),
            ps("Heading1", "Chapter One"),
            p("One cannot know what a man really is by the end of a fortnight."),
        ]
    };
    let mut top = contact();
    top.extend((0..8).map(|_| empty()));
    top.extend(title());
    top.extend(story());
    let mut foot = title();
    foot.extend((0..6).map(|_| empty()));
    foot.extend(contact());
    foot.extend(story());

    for body in [top, foot] {
        let im = Docx::new(&body).import();
        let m = &im.book.metadata;
        assert_eq!(m.title, "SENSE AND SENSIBILITY");
        assert_eq!(m.authors, vec!["Charles Bingley"]);
        assert_eq!(
            outline(&im),
            vec![(SectionKind::Chapter, None, Numbering::Explicit(1))]
        );
        let text = book_text(&im);
        assert!(!text.contains("555") && !text.contains("Tamsin"), "{text}");
        assert_eq!(kinds(&im), vec![WarningKind::TitlePageReplaced]);
    }
}

#[test]
fn usual_gap_per_chapter() {
    // Chapter 1 has no empty paragraphs between paragraphs, so three of
    // them are a scene break. Chapter 2 has one between all paragraphs:
    // that is spacing, and only its typed "***" is a break.
    let one: Vec<String> = [
        "Sie öffnete sich und klappte in den Angeln auf und zu.",
        "Mein Pferd ist verendet und da ist niemand im Dorf, der mir seines leiht.",
        "Wer allerdings mit den Umständen vertraut ist, bemerkt einiges.",
        "Ich habe deine große Wunde aufgefunden; an dieser Blume in deiner Seite gehst du zugrunde.",
        "Schlecht gezahlt, bin ich doch freigebig und hilfsbereit gegenüber den Armen.",
    ]
    .iter()
    .map(|t| p(t))
    .collect();
    let mut body = vec![ps("Heading1", "Kapitel 1")];
    body.extend(one[..3].iter().cloned());
    body.extend([empty(), empty(), empty()]);
    body.extend(one[3..].iter().cloned());
    body.push(ps("Heading1", "Kapitel 2 – Das nächste Dorf"));
    for t in [
        "Und ich nicke der Familie zu.",
        "Immer das Unmögliche vom Arzt verlangen.",
        "***",
        "Dort sitzt er Tage und Jahre.",
        "Nun lebt er nicht mehr lange.",
    ] {
        body.extend([empty(), if t == "***" { pc(t) } else { p(t) }]);
    }
    let im = Docx::new(&body).import();
    let s = &im.book.sections;
    let breaks = |i: usize| {
        s[i].blocks
            .iter()
            .filter(|b| matches!(b, Block::SceneBreak { .. }))
            .count()
    };
    assert_eq!(s[0].blocks.len(), 6, "{:?}", s[0].blocks);
    assert_eq!(s[0].blocks[3], Block::scene_break());
    assert_eq!(breaks(1), 1, "{:?}", s[1].blocks);
    assert_eq!(s[1].blocks.len(), 5);

    let w: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::SceneBreaksFromBlankLines)
        .collect();
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].section, Some(0));
    // The author can find the place: the heading and the words after
    // the break, not a paragraph number.
    assert!(w[0].message.contains("“Kapitel 1”"), "{}", w[0].message);
    assert!(
        w[0].message
            .contains("Ich habe deine große Wunde aufgefunden;"),
        "{}",
        w[0].message
    );
}

#[test]
fn blank_line_breaks_reported_per_chapter() {
    let mut body = vec![];
    for (h, t) in [
        ("Chapter 1", "Mr. Bennet made no answer."),
        ("Chapter 2", "This was invitation enough."),
    ] {
        body.extend([
            ps("Heading1", h),
            p(t),
            p("Bennet replied that he had not."),
            empty(),
            empty(),
            p("Mr. Bennet made no answer."),
        ]);
    }
    let im = Docx::new(&body).import();
    let w: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::SceneBreaksFromBlankLines)
        .map(|w| {
            (
                w.section,
                w.message
                    .contains(&format!("“Chapter {}”", w.section.unwrap_or(9) + 1)),
            )
        })
        .collect();
    assert_eq!(w, vec![(Some(0), true), (Some(1), true)]);
}

/// A table of plain cells, `rows[0]` marked as the header row.
fn table(rows: &[&[&str]]) -> String {
    let mut x = String::from("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/></w:tblPr>");
    for (r, cells) in rows.iter().enumerate() {
        x += "<w:tr>";
        if r == 0 {
            x += "<w:trPr><w:tblHeader/></w:trPr>";
        }
        for c in *cells {
            x += &format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"1800\"/></w:tcPr>{}</w:tc>",
                p(c)
            );
        }
        x += "</w:tr>";
    }
    x + "</w:tbl>"
}

#[test]
fn empty_paragraphs_around_a_table_are_not_scene_breaks() {
    // Word needs a paragraph after a table and authors leave one before
    // it; neither is a scene break.
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Bingley thought her quite beautiful, and danced with her twice."),
        p("Mr. Bennet replied that he had not."),
        empty(),
        table(&[&["Train", "Leaves"], &["41", "11:12 p.m."]]),
        empty(),
        p("But I knew I should persuade you at last."),
        p("Have a little compassion on my nerves."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert!(
        !blocks.iter().any(|b| matches!(b, Block::SceneBreak { .. })),
        "{blocks:?}"
    );
    assert!(blocks.iter().any(|b| matches!(b, Block::Table(_))));
    assert_eq!(kinds(&im), vec![WarningKind::NoTitle]);
}

/// The table block `b`.
fn the_table(b: &Block) -> &Table {
    match b {
        Block::Table(t) => t,
        other => panic!("not a table: {other:?}"),
    }
}

/// The text of every cell, row by row.
fn cell_texts(t: &Table) -> Vec<Vec<String>> {
    t.rows
        .iter()
        .map(|r| r.iter().map(|c| Inline::plain_text(&c.content)).collect())
        .collect()
}

#[test]
fn tables_are_table_blocks() {
    // Rows and cells, the marked header row. A one-column table (a
    // boxed note) is only layout: it keeps its paragraphs.
    let boxed = format!(
        "<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>",
        p("Notice to passengers:"),
        p("You take delight in vexing me.")
    );
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Her report was highly favourable."),
        table(&[
            &["Train", "Halvard St", "Mill Gate"],
            &["41", "11:12 p.m.", "11:34"],
            &["45", "", "12:32"],
        ]),
        p("Mr. Bennet made no answer."),
        boxed,
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert_eq!(blocks.len(), 5, "{blocks:?}");
    assert_eq!(blocks[0], para("Her report was highly favourable."));
    assert_eq!(
        blocks[1],
        Block::Table(Table::new(
            vec![TableColumn::default(); 3],
            1,
            vec![
                vec![
                    TableCell::text("Train"),
                    TableCell::text("Halvard St"),
                    TableCell::text("Mill Gate")
                ],
                vec![
                    TableCell::text("41"),
                    TableCell::text("11:12 p.m."),
                    TableCell::text("11:34")
                ],
                vec![
                    TableCell::text("45"),
                    TableCell::new(vec![]),
                    TableCell::text("12:32")
                ],
            ]
        ))
    );
    assert_eq!(
        blocks[2..],
        [
            para("Mr. Bennet made no answer."),
            para("Notice to passengers:"),
            para("You take delight in vexing me.")
        ]
    );
    let w: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::TableFlattened)
        .map(|w| (w.location, w.message.contains("one column")))
        .collect();
    assert_eq!(w, vec![(13, true)]);
}

#[test]
fn epigraph_page_with_attribution() {
    // Four set-off lines plus a dash attribution, as chapter epigraphs
    // allow. The page gets the section's epigraph, so the theme
    // sets it like one.
    let mut d = Docx::new(&[
        ps("Title", "First Impressions"),
        page_break(),
        pc("For Elizabeth."),
        page_break(),
        ps("Epigraph", "How doth the little crocodile"),
        ps("Epigraph", "What say you, Mary?"),
        ps("Epigraph", "And pour the waters of the Nile"),
        ps("Epigraph", "Bennet made no answer."),
        ps("Epigraph", "— a harbour rhyme"),
        ps("Heading1", "Chapter 1"),
        p("Mr. Bennet made no answer."),
    ]);
    d.styles = style(
        "Epigraph",
        "Epigraph",
        r#"<w:pPr><w:ind w:left="2160"/></w:pPr><w:rPr><w:i/></w:rPr>"#,
    );
    let im = d.import();
    let s = &im.book.sections;
    assert_eq!(
        s.iter().map(|s| s.kind).collect::<Vec<_>>(),
        vec![
            SectionKind::Dedication,
            SectionKind::Epigraph,
            SectionKind::Chapter
        ]
    );
    let line = |t: &str| Inline::text(t);
    assert_eq!(
        s[1].epigraph,
        Some(Epigraph {
            text: vec![
                line("How doth the little crocodile"),
                Inline::LineBreak,
                line("What say you, Mary?"),
                Inline::LineBreak,
                line("And pour the waters of the Nile"),
                Inline::LineBreak,
                line("Bennet made no answer."),
            ],
            attribution: Some("a harbour rhyme".into()),
        })
    );
    assert_eq!(s[1].blocks, vec![]);
    assert!(
        !kinds(&im).contains(&WarningKind::TextBeforeFirstChapter),
        "{:?}",
        im.warnings
    );
}

/// Austen's first six paragraphs (Pride and Prejudice, ch. I): more lines
/// and characters than a guessed epigraph may have.
const AUSTEN_OPENING: [&str; 6] = [
    "It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in want of a wife.",
    "However little known the feelings or views of such a man may be on his first entering a neighbourhood, this truth is so well fixed in the minds of the surrounding families, that he is considered as the rightful property of some one or other of their daughters.",
    "“My dear Mr. Bennet,” said his lady to him one day, “have you heard that Netherfield Park is let at last?”",
    "Mr. Bennet replied that he had not.",
    "“But it is,” returned she; “for Mrs. Long has just been here, and she told me all about it.”",
    "Mr. Bennet made no answer.",
];

/// The paragraphs as epigraph lines.
fn epigraph_lines(lines: &[&str]) -> Vec<Inline> {
    let mut out = Vec::new();
    for (n, l) in lines.iter().enumerate() {
        if n > 0 {
            out.push(Inline::LineBreak);
        }
        out.push(Inline::text(*l));
    }
    out
}

#[test]
fn a_styled_chapter_epigraph_has_no_length_limit() {
    // The Epigraph style is the author's word: six paragraphs and
    // more than 600 characters are still the epigraph. The same text in
    // the Quote style is a guess and keeps the limit: a block quote.
    let mut paras = vec![ps("Heading1", "Chapter I")];
    paras.extend(AUSTEN_OPENING.iter().map(|l| ps("Epigraph", l)));
    paras.push(ps("Epigraph", "— Jane Austen"));
    paras.push(p("Mr. Bennet was so odd a mixture of quick parts."));
    paras.push(ps("Heading1", "Chapter II"));
    paras.extend(AUSTEN_OPENING.iter().map(|l| ps("Quote", l)));
    paras.push(ps("Quote", "— Jane Austen"));
    paras.push(p(
        "Mr. Bennet was among the earliest of those who waited on Mr. Bingley.",
    ));
    let mut d = Docx::new(&paras);
    d.styles = style("Epigraph", "Epigraph", "");
    let im = d.import();
    let s = &im.book.sections;
    assert!(
        AUSTEN_OPENING
            .iter()
            .map(|l| l.chars().count())
            .sum::<usize>()
            > 600
    );
    assert_eq!(
        s[0].epigraph,
        Some(Epigraph {
            text: epigraph_lines(&AUSTEN_OPENING),
            attribution: Some("Jane Austen".into()),
        })
    );
    assert_eq!(
        s[0].blocks,
        vec![para("Mr. Bennet was so odd a mixture of quick parts.")]
    );
    assert_eq!(s[1].epigraph, None);
    assert!(
        matches!(s[1].blocks[0], Block::BlockQuote { .. }),
        "{:?}",
        s[1]
    );
}

#[test]
fn a_styled_chapter_epigraph_needs_no_attribution() {
    // Without a dash line, Epigraph-styled paragraphs under the heading are
    // the epigraph, with no attribution; a Quote-styled opening without
    // one stays a block quote (chapter_epigraphs).
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter I"),
        ps("Epigraph", AUSTEN_OPENING[0]),
        p(AUSTEN_OPENING[1]),
    ]);
    // German Word's custom name; the id is what the paragraphs use.
    d.styles = style("Epigraph", "Motto", "");
    let im = d.import();
    let s = &im.book.sections;
    assert_eq!(
        s[0].epigraph,
        Some(Epigraph {
            text: epigraph_lines(&AUSTEN_OPENING[..1]),
            attribution: None,
        })
    );
    assert_eq!(s[0].blocks, vec![para(AUSTEN_OPENING[1])]);
}

#[test]
fn styled_dedication_and_epigraph_pages() {
    // Kafka's dedication starts with none of the dedication words, and his
    // five sentences are more lines than a guessed epigraph page has; in
    // the Widmung and Motto styles they are a dedication and an epigraph
    // page all the same.
    let kafka = [
        "Vor dem Gesetz steht ein Türhüter.",
        "Zu diesem Türhüter kommt ein Mann vom Lande und bittet um Eintritt in das Gesetz.",
        "Aber der Türhüter sagt, daß er ihm jetzt den Eintritt nicht gewähren könne.",
        "Der Mann überlegt und fragt dann, ob er also später werde eintreten dürfen.",
        "„Es ist möglich“, sagt der Türhüter, „jetzt aber nicht.“",
    ];
    let mut paras = vec![
        ps("Title", "Ein Landarzt"),
        page_break(),
        ps("Widmung", "Meinem Vater"),
        page_break(),
    ];
    paras.extend(kafka.iter().map(|l| ps("Motto", l)));
    paras.push(ps("Motto", "— Franz Kafka"));
    paras.push(ps("Heading1", "Ein Landarzt"));
    paras.push(p(
        "Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor.",
    ));
    let mut d = Docx::new(&paras);
    d.styles = style("Widmung", "Widmung", "") + &style("Motto", "Motto", "");
    let im = d.import();
    let s = &im.book.sections;
    assert_eq!(
        s.iter().map(|s| s.kind).collect::<Vec<_>>(),
        vec![
            SectionKind::Dedication,
            SectionKind::Epigraph,
            SectionKind::Chapter
        ]
    );
    assert_eq!(s[0].blocks, vec![para("Meinem Vater")]);
    assert_eq!(
        s[1].epigraph,
        Some(Epigraph {
            text: epigraph_lines(&kafka),
            attribution: Some("Franz Kafka".into()),
        })
    );
    assert!(
        !kinds(&im).contains(&WarningKind::TextBeforeFirstChapter),
        "{:?}",
        im.warnings
    );

    // A Widmung-styled page whose text is a section name is still the
    // dedication, not a heading ("Widmung" is one).
    let mut d = Docx::new(&[
        ps("Title", "Ein Landarzt"),
        page_break(),
        ps("Widmung", "Widmung"),
        page_break(),
        ps("Motto", "Motto"),
        ps("Motto", "— Franz Kafka"),
        ps("Heading1", "Ein Landarzt"),
        p("Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor."),
    ]);
    d.styles = style("Widmung", "Widmung", "") + &style("Motto", "Motto", "");
    let im = d.import();
    let s = &im.book.sections;
    assert_eq!(
        s.iter()
            .map(|s| (s.kind, s.title.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (SectionKind::Dedication, None),
            (SectionKind::Epigraph, None),
            (SectionKind::Chapter, Some("Ein Landarzt"))
        ]
    );
    assert_eq!(s[0].blocks, vec![para("Widmung")]);

    // Unstyled and centred, the same line is a guess, and "Meinem" is no
    // dedication word: a set-off page, read as an epigraph page.
    let im = Docx::new(&[
        ps("Title", "Ein Landarzt"),
        page_break(),
        pc("Meinem Vater"),
        ps("Heading1", "Ein Landarzt"),
        p("Ich war in großer Verlegenheit: eine dringende Reise stand mir bevor."),
    ])
    .import();
    assert_eq!(im.book.sections[0].kind, SectionKind::Epigraph);
}

#[test]
fn one_title_decision_per_style_under_labels() {
    // A dateline style under every number-only heading: datelines are no
    // chapter titles, with or without a final ".", and the one
    // line in that style that is not a dateline follows the others.
    let mut d = Docx::new(&[
        ps("Heading1", "1"),
        ps("Dateline", "Harbour Office — 11:42 p.m."),
        p("His sisters were fine women, with an air of decided fashion."),
        ps("Heading1", "2"),
        ps("Dateline", "From Pride and Prejudice"),
        p("Jane was so admired, nothing could be like it."),
        ps("Heading1", "3"),
        ps("Dateline", "Harbour Office — 12:05 a.m."),
        p("When she was discontented she fancied herself nervous."),
    ]);
    d.styles = style("Dateline", "Dateline", r#"<w:rPr><w:b/></w:rPr>"#);
    let im = d.import();
    assert_eq!(
        outline(&im),
        (1..=3)
            .map(|n| (SectionKind::Chapter, None, Numbering::Explicit(n)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        im.book.sections[1].blocks[0],
        Block::paragraph(vec![Inline::Strong {
            content: vec![Inline::text("From Pride and Prejudice")]
        }])
    );

    // A title style under every label: titles, the one ending in "." too.
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter One"),
        ps("ChapterTitle", "Netherfield"),
        p("Only think what an establishment it would be for one of them."),
        ps("Heading1", "Chapter Two"),
        ps("ChapterTitle", "First Impressions."),
        p("Sir William had been delighted with him."),
        ps("Heading1", "Chapter Three"),
        ps("ChapterTitle", "Longbourn"),
        p("Such amiable qualities must speak for themselves."),
    ]);
    d.styles = style(
        "ChapterTitle",
        "Chapter Title",
        r#"<w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:i/></w:rPr>"#,
    );
    let titles: Vec<_> = d
        .import()
        .book
        .sections
        .iter()
        .map(|s| s.title.clone())
        .collect();
    assert_eq!(
        titles,
        vec![
            Some("Netherfield".into()),
            Some("First Impressions.".into()),
            Some("Longbourn".into())
        ]
    );

    // Under part labels a date is the part's title as before.
    let im = Docx::new(&[
        ps("Heading1", "Part One"),
        ps("Subtitle", "Berlin, 1989"),
        ps("Heading2", "Chapter 1"),
        p("It was then disclosed in the following manner."),
        ps("Heading1", "Part Two"),
        ps("Subtitle", "1914–1918"),
        ps("Heading2", "Chapter 2"),
        p("Darcy, looking at the eldest Miss Bennet."),
    ])
    .import();
    assert_eq!(
        im.book.sections[0].title.as_deref(),
        Some("Part One: Berlin, 1989")
    );
    assert_eq!(
        im.book.sections[2].title.as_deref(),
        Some("Part Two: 1914–1918")
    );
}

#[test]
fn stanzas_from_paragraph_spacing() {
    // One line per paragraph, the stanza gap made by space before (a
    // "Stanza Start" style) or by direct space after. Lines with
    // the verse's usual spacing stay together.
    let after = |t: &str| {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="Verse"/><w:spacing w:after="280"/></w:pPr>{}</w:p>"#,
            run(t)
        )
    };
    let mut d = Docx::new(&[
        ps("Heading1", "Weather"),
        ps("Verse", "Not handsome enough to dance with!"),
        ps("Verse", "Well, how pleased I am!"),
        ps("StanzaStart", "and would not say"),
        ps("Verse", "First of all, he asked Miss Lucas."),
        p("Bingley was to bring twelve ladies and seven gentlemen with him to the assembly."),
        ps("Verse", "How cheerfully he seems to grin,"),
        after("rain on the slipway,"),
        ps("Verse", "rain in the kettle"),
        ps("Verse", "left out all day."),
    ]);
    d.styles = style(
        "StanzaStart",
        "Verse Stanza Start",
        r#"<w:basedOn w:val="Verse"/><w:pPr><w:spacing w:before="240"/></w:pPr>"#,
    );
    let im = d.import();
    let line = |t: &str| vec![Inline::text(t)];
    let b = &im.book.sections[0].blocks;
    assert_eq!(
        b[0],
        Block::verse(vec![
            line("Not handsome enough to dance with!"),
            line("Well, how pleased I am!"),
            vec![],
            line("and would not say"),
            line("First of all, he asked Miss Lucas."),
        ])
    );
    assert_eq!(
        b[2],
        Block::verse(vec![
            line("How cheerfully he seems to grin,"),
            line("rain on the slipway,"),
            vec![],
            line("rain in the kettle"),
            line("left out all day."),
        ])
    );
}

/// A verse paragraph with a direct left indent in twips (`None`: none).
fn verse_at(style: &str, left: Option<i64>, text: &str) -> String {
    let ind = left.map_or(String::new(), |l| format!(r#"<w:ind w:left="{l}"/>"#));
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/>{ind}</w:pPr>{}</w:p>"#,
        run(text)
    )
}

/// The indent levels of the first block of the book's first section, which
/// must be verse.
fn verse_indents(im: &Imported) -> Vec<u8> {
    let b = &im.book.sections[0].blocks[0];
    let Block::Verse { lines, .. } = b else {
        panic!("not verse: {b:?}");
    };
    (0..lines.len()).map(|i| b.verse_indent(i)).collect()
}

// Carroll's "How doth the little crocodile" (Gutenberg #11), the stanza the
// step names.
const CROC: [&str; 4] = [
    "“How doth the little crocodile",
    "Improve his shining tail,",
    "And pour the waters of the Nile",
    "On every golden scale!",
];

/// A chapter of the stanza, one line a paragraph, with these direct indents.
fn croc(left: [Option<i64>; 4], style: &str) -> Vec<String> {
    let mut paras = vec![ps("Heading1", "The Pool of Tears")];
    for (text, left) in CROC.iter().zip(left) {
        paras.push(verse_at(style, left, text));
    }
    paras
}

#[test]
fn verse_lines_get_their_indent_level() {
    // The step's stanza: every second line in.
    let im = Docx::new(&croc([None, Some(360), None, Some(360)], "Verse")).import();
    assert_eq!(verse_indents(&im), [0, 1, 0, 1]);
    // A verse without indents holds none.
    let flat = Docx::new(&croc([None; 4], "Verse")).import();
    assert_eq!(
        flat.book.sections[0].blocks[0],
        Block::verse(CROC.iter().map(|&t| vec![Inline::text(t)]).collect())
    );
}

#[test]
fn verse_indent_steps_round_and_cap() {
    // 360 twips a level, to the nearest: a ruler wobble under half a level
    // (179) is none, 180 is one, Word's Increase Indent (720) is two.
    let im = Docx::new(&croc([Some(0), Some(179), Some(180), Some(720)], "Verse")).import();
    assert_eq!(verse_indents(&im), [0, 0, 1, 2]);
    // Capped at the model's deepest level, however far out.
    let im = Docx::new(&croc([None, Some(360), Some(720), Some(100_000)], "Verse")).import();
    assert_eq!(
        verse_indents(&im),
        [0, 1, 2, setzer_model::MAX_VERSE_INDENT]
    );
}

#[test]
fn verse_indent_is_beyond_the_style_and_the_least_indented_line() {
    // The style "Poem" is indented as a whole: only what is beyond its 720
    // counts, and a line left of it is flush (no negative levels).
    let mut d = Docx::new(&croc([Some(0), None, Some(1080), Some(1440)], "Poem"));
    d.styles = style(
        "Poem",
        "Poem",
        r#"<w:basedOn w:val="Verse"/><w:pPr><w:ind w:left="720"/></w:pPr>"#,
    );
    assert_eq!(verse_indents(&d.import()), [0, 0, 1, 2]);
    // A verse indented directly as a whole (every line 720) is flush too,
    // its indented lines one level in.
    let im = Docx::new(&croc(
        [Some(720), Some(1080), Some(720), Some(1080)],
        "Verse",
    ))
    .import();
    assert_eq!(verse_indents(&im), [0, 1, 0, 1]);
    // `w:start` is the left indent too.
    let mut paras = croc([None; 4], "Verse");
    paras[2] = paras[2].replace(
        r#"<w:pStyle w:val="Verse"/>"#,
        r#"<w:pStyle w:val="Verse"/><w:ind w:start="360"/>"#,
    );
    assert_eq!(verse_indents(&Docx::new(&paras).import()), [0, 1, 0, 0]);
}

#[test]
fn verse_indent_applies_to_a_stanza_in_one_paragraph() {
    // A paragraph with line breaks is a stanza; its indent is all its lines',
    // and the gap between stanzas is flush.
    let stanza = |left: Option<i64>, lines: &[&str]| {
        let ind = left.map_or(String::new(), |l| format!(r#"<w:ind w:left="{l}"/>"#));
        let runs: Vec<String> = lines.iter().map(|t| run(t)).collect();
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="Verse"/>{ind}</w:pPr>{}</w:p>"#,
            runs.join("<w:r><w:br/></w:r>")
        )
    };
    let im = Docx::new(&[
        ps("Heading1", "The Pool of Tears"),
        stanza(None, &CROC[..3]),
        stanza(Some(360), &CROC[1..]),
    ])
    .import();
    assert_eq!(verse_indents(&im), [0, 0, 0, 0, 1, 1, 1]);
}

#[test]
fn verse_indent_survives_absurd_numbers() {
    // A damaged file's indent and spacing saturate instead of overflowing
    // (a panic in debug builds): the biggest and smallest numbers read.
    let big = i64::MAX;
    let im = Docx::new(&croc([None, Some(big), Some(i64::MIN), Some(360)], "Verse")).import();
    assert_eq!(verse_indents(&im).len(), 4);
    let mut d = Docx::new(&croc([None, Some(big), None, Some(360)], "Poem"));
    d.styles = style(
        "Poem",
        "Poem",
        &format!(
            r#"<w:basedOn w:val="Verse"/><w:pPr><w:ind w:left="{}"/><w:spacing w:before="{big}" w:after="{big}"/></w:pPr>"#,
            i64::MIN
        ),
    );
    assert_eq!(verse_indents(&d.import()).len(), 4);
    let mut paras = croc([None; 4], "Verse");
    paras[1] = paras[1].replace(
        r#"<w:pStyle w:val="Verse"/>"#,
        &format!(r#"<w:pStyle w:val="Verse"/><w:spacing w:before="{big}" w:after="{big}"/>"#),
    );
    assert_eq!(verse_indents(&Docx::new(&paras).import()).len(), 5);
}

#[test]
fn verse_indent_reads_a_first_line_indent_on_a_one_line_paragraph() {
    // Word turns a Tab at the start of a line into a first-line indent.
    let mut paras = croc([None; 4], "Verse");
    for k in [2, 4] {
        paras[k] = paras[k].replace(
            r#"<w:pStyle w:val="Verse"/>"#,
            r#"<w:pStyle w:val="Verse"/><w:ind w:firstLine="360"/>"#,
        );
    }
    assert_eq!(verse_indents(&Docx::new(&paras).import()), [0, 1, 0, 1]);
    // A hanging indent starts the line left of the left indent by that much.
    let mut paras = croc([None; 4], "Verse");
    paras[2] = paras[2].replace(
        r#"<w:pStyle w:val="Verse"/>"#,
        r#"<w:pStyle w:val="Verse"/><w:ind w:left="720" w:hanging="360"/>"#,
    );
    assert_eq!(verse_indents(&Docx::new(&paras).import()), [0, 1, 0, 0]);
}

#[test]
fn closing_end_mark_is_dropped() {
    // A manuscript's closing "END" (in a Scene Break style) or a
    // centred "Ende" after empty lines is not part of the book.
    for (mark, under_blanks) in [
        (ps("SceneBreak", "END"), false),
        (pc("Ende"), true),
        (pc("— The End —"), false),
        (p("FIN."), false),
    ] {
        let mut body = vec![
            ps("Heading1", "Chapter One"),
            p("She was a woman of mean understanding, little information, and uncertain temper."),
            p("It is a truth universally acknowledged."),
        ];
        if under_blanks {
            body.extend((0..3).map(|_| empty()));
        }
        body.push(mark);
        let im = Docx::new(&body).import();
        assert_eq!(
            im.book.sections[0].blocks,
            vec![
                para(
                    "She was a woman of mean understanding, little information, and uncertain temper."
                ),
                para("It is a truth universally acknowledged."),
            ]
        );
        let w = im
            .warnings
            .iter()
            .find(|w| w.kind == WarningKind::EndMarkRemoved)
            .expect("an EndMarkRemoved warning");
        assert_eq!((w.location, w.section), (body.len(), Some(0)));
    }

    // Not at the very end, or more than the mark: kept.
    for body in [
        vec![
            ps("Heading1", "Chapter One"),
            p("Words."),
            pc("END"),
            p("Bennet protested against any description of finery."),
        ],
        vec![
            ps("Heading1", "Chapter One"),
            p("Words."),
            p("Lizzy is not a bit better than the others."),
        ],
        vec![ps("Heading1", "The End"), p("Words.")],
    ] {
        let im = Docx::new(&body).import();
        assert!(!kinds(&im).contains(&WarningKind::EndMarkRemoved));
    }
    let im = Docx::new(&[
        ps("Heading1", "Chapter One"),
        p("Words."),
        p("Lizzy is not a bit better than the others."),
    ])
    .import();
    assert_eq!(im.book.sections[0].blocks.len(), 2);
}

#[test]
fn title_typed_in_capitals() {
    // the Title paragraph in capitals, the document properties in
    // normal case: the metadata (store listings) take the cased title.
    let body = || {
        vec![
            ps("Title", "PRIDE AND PREJUDICE"),
            p("by Charlotte Lucas"),
            page_break(),
            ps("Heading1", "Chapter One"),
            p("Bennet had seen her eldest daughter much admired by the Netherfield party."),
        ]
    };
    let mut d = Docx::new(&body());
    d.core = Some(core("Pride and Prejudice", "Charlotte Lucas", ""));
    let im = d.import();
    assert_eq!(im.book.metadata.title, "Pride and Prejudice");
    let replaced = |im: &Imported| {
        im.warnings
            .iter()
            .find(|w| w.kind == WarningKind::TitlePageReplaced)
            .map(|w| w.message.clone())
            .unwrap_or_default()
    };
    assert!(
        replaced(&im).contains("“Pride and Prejudice”"),
        "{}",
        replaced(&im)
    );

    // Without a cased title to go by, the capitals stay as typed, and the
    // warning says so. A different properties title is not a spelling.
    for core_title in [None, Some("Draft 3")] {
        let mut d = Docx::new(&body());
        d.core = core_title.map(|t| core(t, "", ""));
        let im = d.import();
        assert_eq!(im.book.metadata.title, "PRIDE AND PREJUDICE");
        assert!(replaced(&im).contains("capitals"), "{}", replaced(&im));
    }
}

#[test]
fn subtitle_from_a_replaced_title_page() {
    // an unstyled title page of title, genre line and author (no
    // "von", but the document's author): the middle line is the subtitle.
    let story = || {
        vec![
            page_break(),
            ps("Heading1", "Erstes Kapitel"),
            p(
                "Title: Ein Landarzt Subtitle: Kleine Erzählungen Author: Franz Kafka Lang: de Copyright: Gemeinfrei.",
            ),
        ]
    };
    let mut body = vec![pc("Das nächste Dorf"), empty(), pc("Erzählung")];
    body.extend([empty(), empty(), pc("Jane Bennet")]);
    body.extend(story());
    let mut d = Docx::new(&body);
    d.core = Some(core("", "Jane Bennet", "de"));
    let im = d.import();
    let m = &im.book.metadata;
    assert_eq!(m.title, "Das nächste Dorf");
    assert_eq!(m.subtitle.as_deref(), Some("Erzählung"));
    assert_eq!(m.authors, vec!["Jane Bennet"]);

    // The author only in the document properties: the line under the
    // title is not the author, so it is the subtitle.
    let mut body = vec![pc("Das nächste Dorf"), pc("Erzählung")];
    body.extend(story());
    let mut d = Docx::new(&body);
    d.core = Some(core("", "Jane Bennet", "de"));
    assert_eq!(
        d.import().book.metadata.subtitle.as_deref(),
        Some("Erzählung")
    );

    // Title and one more line, nothing says which is the author: no
    // subtitle, and the warning names the line that was left out.
    let mut body = vec![pc("Das nächste Dorf"), pc("Jane Bennet")];
    body.extend(story());
    let im = Docx::new(&body).import();
    assert_eq!(im.book.metadata.subtitle, None);
    let w = im
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::TitlePageReplaced)
        .unwrap();
    assert!(w.message.contains("“Jane Bennet”"), "{}", w.message);

    // A styled title page: a line in no title style is named too.
    let mut body = vec![
        ps("Title", "Das nächste Dorf"),
        ps("Subtitle", "Erzählung"),
        pc("Fassung vom Mai"),
    ];
    body.extend(story());
    let im = Docx::new(&body).import();
    assert_eq!(im.book.metadata.subtitle.as_deref(), Some("Erzählung"));
    let w = im
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::TitlePageReplaced)
        .unwrap();
    assert!(w.message.contains("“Fassung vom Mai”"), "{}", w.message);
}

/// A table cell: `w:tc` around the given paragraphs.
fn tc(paras: &[String]) -> String {
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"1800\"/></w:tcPr>{}</w:tc>",
        paras.concat()
    )
}

/// A paragraph with direct paragraph properties (`ppr` is the inside of
/// `w:pPr`).
fn pp(ppr: &str, text: &str) -> String {
    format!("<w:p><w:pPr>{ppr}</w:pPr>{}</w:p>", run(text))
}

/// A table row of cells.
fn tr(cells: &[String]) -> String {
    format!("<w:tr>{}</w:tr>", cells.concat())
}

#[test]
fn cell_paragraph_properties_stay_in_the_cell() {
    // Indents, first-line indents, space and styles on the paragraphs of a
    // cell do nothing outside it: an indent on both sides makes a block
    // quote of a body paragraph, a Heading 2 style a heading, a Quote style
    // a quote.
    let indent = r#"<w:spacing w:before="240" w:after="240"/><w:ind w:left="720" w:right="720" w:firstLine="360"/>"#;
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Bennet was quite disconcerted."),
        format!(
            "<w:tbl>{}{}</w:tbl>",
            tr(&[tc(&[ps("Heading2", "Train")]), tc(&[ps("Quote", "Leaves")])]),
            tr(&[tc(&[pp(indent, "41")]), tc(&[pp(indent, "11:12 p.m.")])]),
        ),
        p("Mr. Bennet made no answer."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert_eq!(
        blocks,
        &vec![
            para("Bennet was quite disconcerted."),
            Block::Table(Table::new(
                vec![TableColumn::default(); 2],
                0,
                vec![
                    vec![TableCell::text("Train"), TableCell::text("Leaves")],
                    vec![TableCell::text("41"), TableCell::text("11:12 p.m.")],
                ]
            )),
            para("Mr. Bennet made no answer."),
        ]
    );
    assert_eq!(im.book.sections.len(), 1);
    assert_eq!(kinds(&im), vec![WarningKind::NoTitle]);
}

/// A cell paragraph aligned `jc` ("right", "center").
fn pj(jc: &str, text: &str) -> String {
    pp(&format!(r#"<w:jc w:val="{jc}"/>"#), text)
}

/// A body of "Chapter 1", a paragraph, the table rows and a paragraph.
fn with_table(rows: &[String]) -> Imported {
    Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        format!("<w:tbl>{}</w:tbl>", rows.concat()),
        p("After."),
    ])
    .import()
}

/// A row marked as a header row.
fn header_row(cells: &[String]) -> String {
    format!(
        "<w:tr><w:trPr><w:tblHeader/></w:trPr>{}</w:tr>",
        cells.concat()
    )
}

#[test]
fn column_alignment_is_the_most_common_in_the_body() {
    // Right and center count for the column; empty cells do not; a tie goes
    // left.
    let im = with_table(&[
        tr(&[
            tc(&[p("Name")]),
            tc(&[pj("right", "Words")]),
            tc(&[p("Mid")]),
            tc(&[p("Mixed")]),
        ]),
        tr(&[
            tc(&[p("a")]),
            tc(&[pj("right", "4")]),
            tc(&[pj("center", "x")]),
            tc(&[pj("right", "1")]),
        ]),
        tr(&[
            tc(&[p("b")]),
            tc(&[pj("right", "19")]),
            tc(&[pj("center", "y")]),
            tc(&[p("2")]),
        ]),
        tr(&[
            tc(&[p("c")]),
            tc(&[empty()]),
            tc(&[pj("center", "z")]),
            tc(&[pj("right", "3")]),
        ]),
        tr(&[tc(&[p("d")]), tc(&[p("5")]), tc(&[p("w")]), tc(&[p("4")])]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    // Header rows come from `w:tblHeader` only: none was marked.
    assert_eq!(t.header_rows, 0);
    let aligns: Vec<_> = t.columns.iter().map(|c| c.align).collect();
    assert_eq!(
        aligns,
        [
            ColumnAlign::Left,
            ColumnAlign::Right,
            ColumnAlign::Center,
            ColumnAlign::Left
        ]
    );
}

#[test]
fn a_header_row_does_not_vote_for_alignment() {
    // "Words" is right-aligned over a column of left-aligned numbers.
    let im = with_table(&[
        header_row(&[tc(&[p("Name")]), tc(&[pj("right", "Words")])]),
        tr(&[tc(&[p("a")]), tc(&[p("4")])]),
        tr(&[tc(&[p("b")]), tc(&[p("19")])]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.header_rows, 1);
    assert_eq!(t.columns[1].align, ColumnAlign::Left);
}

#[test]
fn header_rows_are_the_leading_marked_rows() {
    // Two marked rows lead; a marked row after an unmarked one is not a
    // header (a repeated header must start at the first row).
    let marked = |a: &str, b: &str| header_row(&[tc(&[p(a)]), tc(&[p(b)])]);
    let im = with_table(&[
        marked("Station", "Times"),
        marked("", "Dep."),
        tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])]),
        marked("Mill Gate", "12:37"),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.header_rows, 2);
    assert_eq!(t.rows.len(), 4);
}

#[test]
fn a_cell_of_several_paragraphs_is_lines_of_one_cell() {
    // Empty paragraphs at either end of a cell are nothing; a bold run the
    // author typed stays bold; a header gets no bold of its own.
    let bold = r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Heavy</w:t></w:r></w:p>"#;
    let im = with_table(&[
        header_row(&[tc(&[p("Said")]), tc(&[p("Line")])]),
        tr(&[
            tc(&[empty(), p("first"), p("second"), empty()]),
            tc(&[bold.into()]),
        ]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        t.rows[0][0].content,
        vec![Inline::text("Said")],
        "no Strong for a header"
    );
    assert_eq!(
        t.rows[1][0].content,
        vec![
            Inline::text("first"),
            Inline::LineBreak,
            Inline::text("second")
        ]
    );
    assert_eq!(
        t.rows[1][1].content,
        vec![Inline::Strong {
            content: vec![Inline::text("Heavy")]
        }]
    );
}

#[test]
fn a_footnote_in_a_cell_stays_in_the_cell() {
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        format!(
            "<w:tbl>{}</w:tbl>",
            tr(&[
                tc(&[format!(
                    r#"<w:p>{}<w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
                    run("Quay")
                )]),
                tc(&[p("12:30")])
            ])
        ),
        p("After."),
    ]);
    d.footnotes = Some(format!(
        r#"<w:footnote w:id="1">{}</w:footnote>"#,
        p("A footnote.")
    ));
    let im = d.import();
    let t = the_table(&im.book.sections[0].blocks[0]);
    assert_eq!(
        t.rows[0][0].content,
        vec![
            Inline::text("Quay"),
            Inline::Footnote {
                blocks: vec![para("A footnote.")]
            }
        ]
    );
}

#[test]
fn a_table_in_a_footnote_is_joined_by_row() {
    // A footnote holds blocks, not a table: its rows are lines of text.
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        format!(
            r#"<w:p>{}<w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
            run("Text")
        ),
    ]);
    d.footnotes = Some(format!(
        r#"<w:footnote w:id="1"><w:tbl>{}</w:tbl></w:footnote>"#,
        tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])])
    ));
    let im = d.import();
    assert_eq!(
        im.book.sections[0].blocks,
        vec![Block::paragraph(vec![
            Inline::text("Text"),
            Inline::Footnote {
                blocks: vec![para("Quay · 12:30")]
            }
        ])]
    );
}

#[test]
fn a_table_first_in_a_chapter_and_a_table_with_no_text() {
    // A table can open a chapter. A table with nothing in it is dropped.
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        format!(
            "<w:tbl>{}</w:tbl>",
            tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])])
        ),
        p("After."),
        format!("<w:tbl>{}</w:tbl>", tr(&[tc(&[empty()]), tc(&[empty()])])),
        p("Mr. Bennet made no answer."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    assert!(matches!(blocks[0], Block::Table(_)));
}

#[test]
fn a_file_that_ends_inside_a_table_keeps_the_table() {
    let xml = format!(
        "<w:tbl>{}{}",
        tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])]),
        "<w:tr><w:tc><w:p><w:r><w:t>Mill"
    );
    let im = Docx::new(&[ps("Heading1", "Chapter 1"), p("Before."), xml]).import();
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(cell_texts(t)[0], ["Quay", "12:30"]);
}

#[test]
fn one_empty_paragraph_between_two_tables_is_not_a_scene_break() {
    // Word merges two tables unless a paragraph separates them.
    let one = || {
        format!(
            "<w:tbl>{}</w:tbl>",
            tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])])
        )
    };
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        one(),
        empty(),
        one(),
        empty(),
        p("After."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert!(
        matches!(blocks[..], [_, Block::Table(_), Block::Table(_), _]),
        "{blocks:?}"
    );
}

#[test]
fn two_empty_paragraphs_after_a_table_are_a_scene_break() {
    // One is the table's spacing; the second is the author's.
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        format!(
            "<w:tbl>{}</w:tbl>",
            tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])])
        ),
        empty(),
        empty(),
        p("After."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert!(
        matches!(
            blocks[..],
            [_, Block::Table(_), Block::SceneBreak { .. }, _]
        ),
        "{blocks:?}"
    );
}

#[test]
fn a_table_is_not_a_title_page_and_not_text_to_the_heading_tests() {
    // A table before the first heading is kept, in a section of its own;
    // nothing in it is read as the title, the author or a contact block.
    let im = Docx::new(&[
        pc("FIRST IMPRESSIONS"),
        pc("by Jane Doe"),
        page_break(),
        format!(
            "<w:tbl>{}{}</w:tbl>",
            tr(&[tc(&[p("Jane Doe")]), tc(&[p("jane@example.com")])]),
            tr(&[tc(&[p("approx. 4,000 words")]), tc(&[p("Chapter One")])]),
        ),
        page_break(),
        ps("Heading1", "Chapter One"),
        p("His character was decided."),
    ])
    .import();
    let other = im
        .book
        .sections
        .iter()
        .find(|s| s.kind == SectionKind::Other)
        .expect("the page with the table");
    assert!(matches!(other.blocks[..], [Block::Table(_)]));
    assert_eq!(im.book.metadata.title, "FIRST IMPRESSIONS");
    assert_eq!(im.book.metadata.authors, vec!["Jane Doe"]);
    assert!(
        kinds(&im).contains(&WarningKind::TextBeforeFirstChapter),
        "{:?}",
        kinds(&im)
    );
}

fn simplified(im: &Imported) -> Vec<(usize, String)> {
    im.warnings
        .iter()
        .filter(|w| w.kind == WarningKind::TableSimplified)
        .map(|w| (w.location, w.message.clone()))
        .collect()
}

#[test]
fn several_paragraphs_and_a_nested_table_give_one_table_simplified() {
    let nested = format!(
        "<w:tc>{}<w:tbl>{}</w:tbl>{}</w:tc>",
        p("Outer"),
        tr(&[tc(&[p("in")]), tc(&[p("side")])]),
        empty()
    );
    let im = with_table(&[
        tr(&[tc(&[p("a"), p("b")]), nested]),
        tr(&[tc(&[p("c")]), tc(&[p("d")])]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    // The nested table is kept as lines of the cell, a row a line.
    assert_eq!(
        t.rows[0][1].content,
        vec![
            Inline::text("Outer"),
            Inline::LineBreak,
            Inline::text("in · side")
        ]
    );
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].0, 3, "the first paragraph of the table");
    assert!(w[0].1.contains("several paragraphs") && w[0].1.contains("inside a cell"));
}

#[test]
fn a_plain_table_gives_no_warning_and_a_table_with_no_text_is_reported() {
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        format!(
            "<w:tbl>{}</w:tbl>",
            tr(&[tc(&[p("Quay")]), tc(&[p("12:30")])])
        ),
        p("Between."),
        format!("<w:tbl>{}</w:tbl>", tr(&[tc(&[empty()]), tc(&[empty()])])),
        p("After."),
    ])
    .import();
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("no text"), "{w:?}");
    assert_eq!(w[0].0, 6);
    let blocks = &im.book.sections[0].blocks;
    assert_eq!(blocks.len(), 4, "{blocks:?}");
}

#[test]
fn a_table_on_a_styled_title_page() {
    // A Title paragraph and a table on page 1: the page is replaced by a
    // generated one, and the table's text is named in the warning.
    let im = Docx::new(&[
        ps("Title", "Ein Landarzt"),
        table(&[&["Novelle", "Franz Kafka"]]),
        page_break(),
        ps("Heading1", "Kapitel 1"),
        p("So aus der Entfernung."),
    ])
    .import();
    let blocks: usize = im.book.sections.iter().map(|s| s.blocks.len()).sum();
    assert_eq!(blocks, 1, "the chapter paragraph; no table block");
    let message = im
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::TitlePageReplaced)
        .map(|w| w.message.clone())
        .unwrap_or_default();
    assert!(message.contains("“Novelle · Franz Kafka”"), "{message}");
}

#[test]
fn a_contact_table_on_a_styled_title_page_is_not_named() {
    let im = Docx::new(&[
        ps("Title", "Ein Landarzt"),
        table(&[&["Franz Kafka", "franz@example.com"]]),
        page_break(),
        ps("Heading1", "Kapitel 1"),
        p("So aus der Entfernung."),
    ])
    .import();
    let w = im
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::TitlePageReplaced)
        .expect("title page replaced");
    assert!(!w.message.contains("Not in the book"), "{}", w.message);
}

#[test]
fn a_table_of_zero_width_spaces_has_no_text() {
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        table(&[&["\u{200B}", "\u{FEFF}"]]),
        p("After."),
    ])
    .import();
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("no text"), "{w:?}");
    let blocks = &im.book.sections[0].blocks;
    assert!(!blocks.iter().any(|b| matches!(b, Block::Table(_))));
}

#[test]
fn one_empty_paragraph_between_a_table_and_a_layout_table_is_spacing() {
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        table(&[&["Train", "Leaves"], &["41", "11:12 p.m."]]),
        empty(),
        format!("<w:tbl>{}</w:tbl>", tr(&[tc(&[p("A box around text.")])])),
        p("After."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert!(
        !blocks.iter().any(|b| matches!(b, Block::SceneBreak { .. })),
        "{blocks:?}"
    );
}

// ---------------------------------------------------------------------
// Merged cells, header growth, spanning alignment, degrades.

/// A cell whose `w:tcPr` holds `props` besides the width.
fn tcp(props: &str, paras: &[String]) -> String {
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"1800\"/>{props}</w:tcPr>{}</w:tc>",
        paras.concat()
    )
}

/// A cell of plain text.
fn cell(text: &str) -> String {
    tc(&[p(text)])
}

/// A cell over `n` columns (`w:gridSpan`).
fn span(n: u32, text: &str) -> String {
    tcp(&format!(r#"<w:gridSpan w:val="{n}"/>"#), &[p(text)])
}

/// The top of a vertical merge.
fn restart(text: &str) -> String {
    tcp(r#"<w:vMerge w:val="restart"/>"#, &[p(text)])
}

/// A cell the merge above reaches into: empty, as Word writes it.
fn cont() -> String {
    tcp("<w:vMerge/>", &[empty()])
}

/// A legacy horizontal merge: `kind` is "restart" or "continue".
fn hmerge(kind: &str, text: &str) -> String {
    let props = if kind == "restart" {
        r#"<w:hMerge w:val="restart"/>"#
    } else {
        "<w:hMerge/>"
    };
    tcp(props, &[if text.is_empty() { empty() } else { p(text) }])
}

/// A row with `w:trPr` content.
fn trp(props: &str, cells: &[String]) -> String {
    format!("<w:tr><w:trPr>{props}</w:trPr>{}</w:tr>", cells.concat())
}

type Placed = (usize, usize, usize, usize, String);

/// `(row, column, rowspan, colspan, text)` of every cell as the model lays
/// the table out: what Word shows, when the importer placed the cells where
/// Word has them.
fn placed(t: &Table) -> Vec<Placed> {
    t.layout()
        .cells
        .iter()
        .map(|c| {
            (
                c.row,
                c.column,
                c.rowspan,
                c.colspan,
                Inline::plain_text(&c.content()),
            )
        })
        .collect()
}

/// The cells the layout had to make up (a short row, a gap).
fn made_up(t: &Table) -> usize {
    t.layout()
        .cells
        .iter()
        .filter(|c| c.source.is_none())
        .count()
}

fn pl(row: usize, col: usize, rs: usize, cs: usize, text: &str) -> Placed {
    (row, col, rs, cs, text.to_string())
}

#[test]
fn a_grid_span_is_a_colspan() {
    let im = with_table(&[
        tr(&[span(2, "Banner")]),
        tr(&[cell("A"), cell("B")]),
        tr(&[cell("C"), cell("D")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.columns.len(), 2);
    assert_eq!(
        cell_texts(t),
        [vec!["Banner"], vec!["A", "B"], vec!["C", "D"]]
    );
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 2, "Banner"),
            pl(1, 0, 1, 1, "A"),
            pl(1, 1, 1, 1, "B"),
            pl(2, 0, 1, 1, "C"),
            pl(2, 1, 1, 1, "D"),
        ]
    );
    assert_eq!(made_up(t), 0);
    assert!(simplified(&im).is_empty());
}

#[test]
fn a_vertical_merge_chain_is_a_rowspan_and_the_covered_cells_are_not_listed() {
    let im = with_table(&[
        tr(&[restart("A"), cell("x")]),
        tr(&[cont(), cell("y")]),
        tr(&[cont(), cell("z")]),
        tr(&[cell("B"), cell("w")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        cell_texts(t),
        [vec!["A", "x"], vec!["y"], vec!["z"], vec!["B", "w"]]
    );
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 3, 1, "A"),
            pl(0, 1, 1, 1, "x"),
            pl(1, 1, 1, 1, "y"),
            pl(2, 1, 1, 1, "z"),
            pl(3, 0, 1, 1, "B"),
            pl(3, 1, 1, 1, "w"),
        ]
    );
    assert_eq!(made_up(t), 0);
    assert!(simplified(&im).is_empty());
}

#[test]
fn a_restart_alone_and_a_chain_to_the_last_row() {
    // A restart with nothing below it is a plain cell and no warning (Word
    // leaves one when a merge is split again); a chain may end at the last
    // row.
    let im = with_table(&[
        tr(&[restart("Alone"), cell("x")]),
        tr(&[cell("B"), cell("y")]),
        tr(&[restart("Tail"), cell("z")]),
        tr(&[cont(), cell("w")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 1, "Alone"),
            pl(0, 1, 1, 1, "x"),
            pl(1, 0, 1, 1, "B"),
            pl(1, 1, 1, 1, "y"),
            pl(2, 0, 2, 1, "Tail"),
            pl(2, 1, 1, 1, "z"),
            pl(3, 1, 1, 1, "w"),
        ]
    );
    assert!(simplified(&im).is_empty());
}

#[test]
fn a_continue_cell_with_nothing_to_continue_is_an_ordinary_cell() {
    // No restart above it, in the first row, at another column, at another
    // width: the cell is kept as it is and the table is said to be simplified.
    let tables: [(&str, Vec<String>); 4] = [
        (
            "plain cell above",
            vec![tr(&[cell("A"), cell("B")]), tr(&[cont(), cell("C")])],
        ),
        (
            "first row",
            vec![tr(&[cont(), cell("x")]), tr(&[cell("A"), cell("B")])],
        ),
        (
            "another column",
            vec![
                tr(&[restart("A"), cell("B"), cell("C")]),
                tr(&[cell("P"), cont(), cell("Q")]),
            ],
        ),
        (
            "another width",
            vec![
                tr(&[restart("A"), cell("B")]),
                tr(&[tcp(r#"<w:gridSpan w:val="2"/><w:vMerge/>"#, &[empty()])]),
            ],
        ),
    ];
    for (what, rows) in tables {
        let im = with_table(&rows);
        let t = the_table(&im.book.sections[0].blocks[1]);
        assert!(
            placed(t).iter().all(|c| c.2 == 1),
            "{what}: no rowspan: {:?}",
            placed(t)
        );
        assert_eq!(t.rows.len(), 2, "{what}");
        let w = simplified(&im);
        assert_eq!(w.len(), 1, "{what}: {w:?}");
        assert!(w[0].1.contains("nothing to merge into"), "{what}: {w:?}");
    }
    // The cell stays where it was written.
    let im = with_table(&[
        tr(&[restart("A"), cell("B"), cell("C")]),
        tr(&[cell("P"), cont(), cell("Q")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(cell_texts(t)[1], ["P", "", "Q"]);
}

#[test]
fn text_in_a_covered_cell_goes_to_the_merged_cell() {
    // Word leaves a covered cell empty; a file that does not must not lose
    // the text.
    let stray = tcp("<w:vMerge/>", &[p("stray")]);
    let im = with_table(&[
        tr(&[restart("A"), cell("x")]),
        tr(&[stray, cell("y")]),
        tr(&[
            hmerge("restart", "Wide"),
            hmerge("continue", "beside"),
            cell("C"),
        ]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        t.rows[0][0].content,
        vec![Inline::text("A"), Inline::LineBreak, Inline::text("stray")]
    );
    assert_eq!(t.rows[0][0].rowspan, 2);
    assert_eq!(
        t.rows[2][0].content,
        vec![
            Inline::text("Wide"),
            Inline::LineBreak,
            Inline::text("beside")
        ]
    );
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("covered cell"), "{w:?}");
}

#[test]
fn a_span_that_starts_in_the_header_grows_it_to_its_last_row() {
    // "Station" is drawn over two rows, only the first marked as a header
    // row: the whole span is the header, or the layout would cut it.
    let im = with_table(&[
        header_row(&[restart("Station"), span(2, "Outward")]),
        tr(&[cont(), cell("Dep."), cell("Arr.")]),
        tr(&[restart("Quay"), cell("12:30"), cell("12:37")]),
        tr(&[cont(), cell("13:30"), cell("13:37")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.header_rows, 2);
    assert_eq!(t.layout().header_rows, 2);
    assert_eq!(
        placed(t)[..4],
        [
            pl(0, 0, 2, 1, "Station"),
            pl(0, 1, 1, 2, "Outward"),
            pl(1, 1, 1, 1, "Dep."),
            pl(1, 2, 1, 1, "Arr."),
        ]
    );
    assert_eq!(placed(t)[4], pl(2, 0, 2, 1, "Quay"));
    assert_eq!(made_up(t), 0);
}

#[test]
fn header_growth_follows_a_span_that_starts_in_the_grown_part() {
    // Row 0 is marked; "A" reaches row 1, where "y" starts and reaches row 2.
    let im = with_table(&[
        header_row(&[restart("A"), cell("x")]),
        tr(&[cont(), restart("y")]),
        tr(&[cell("p"), cont()]),
        tr(&[cell("q"), cell("z")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.header_rows, 3);
    // A span in the body does not touch the header.
    let im = with_table(&[
        header_row(&[cell("A"), cell("B")]),
        tr(&[restart("p"), cell("x")]),
        tr(&[cont(), cell("y")]),
    ]);
    assert_eq!(the_table(&im.book.sections[0].blocks[1]).header_rows, 1);
}

#[test]
fn the_width_is_the_widest_row_and_absurd_spans_are_cut() {
    let im = with_table(&[tr(&[span(5, "Wide")]), tr(&[cell("A"), cell("B")])]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.columns.len(), 5);
    assert_eq!(placed(t)[0], pl(0, 0, 1, 5, "Wide"));
    assert_eq!(cell_texts(t)[1][..2], ["A", "B"]);

    let im = with_table(&[tr(&[span(1_000_000, "Huge")]), tr(&[cell("A"), cell("B")])]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.columns.len(), 64);
    assert_eq!(t.rows[0][0].colspan, 64);

    // 0 and negative spans are 1.
    let zero = tcp(r#"<w:gridSpan w:val="0"/>"#, &[p("Z")]);
    let neg = tcp(r#"<w:gridSpan w:val="-3"/>"#, &[p("N")]);
    let im = with_table(&[tr(&[zero, neg])]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(placed(t), [pl(0, 0, 1, 1, "Z"), pl(0, 1, 1, 1, "N")]);
}

#[test]
fn a_legacy_horizontal_merge_is_a_colspan() {
    let im = with_table(&[
        tr(&[hmerge("restart", "Wide"), hmerge("continue", ""), cell("C")]),
        tr(&[cell("A"), cell("B"), cell("D")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 2, "Wide"),
            pl(0, 2, 1, 1, "C"),
            pl(1, 0, 1, 1, "A"),
            pl(1, 1, 1, 1, "B"),
            pl(1, 2, 1, 1, "D"),
        ]
    );
    assert!(simplified(&im).is_empty());

    // A continue cell with no restart beside it is an ordinary cell.
    let im = with_table(&[
        tr(&[hmerge("continue", "x"), cell("y"), cell("z")]),
        tr(&[cell("A"), cell("B"), cell("D")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(cell_texts(t)[0], ["x", "y", "z"]);
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("nothing to merge into"), "{w:?}");
}

#[test]
fn grid_before_and_after_are_empty_cells_at_either_end_of_the_row() {
    let im = with_table(&[
        tr(&[cell("A"), cell("B"), cell("C")]),
        trp(r#"<w:gridBefore w:val="1"/>"#, &[cell("D"), cell("E")]),
        trp(r#"<w:gridAfter w:val="1"/>"#, &[cell("F"), cell("G")]),
        trp(
            r#"<w:gridBefore w:val="1"/><w:gridAfter w:val="1"/>"#,
            &[cell("H")],
        ),
        trp(r#"<w:gridBefore w:val="2"/>"#, &[cell("I")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.columns.len(), 3);
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 1, "A"),
            pl(0, 1, 1, 1, "B"),
            pl(0, 2, 1, 1, "C"),
            pl(1, 0, 1, 1, ""),
            pl(1, 1, 1, 1, "D"),
            pl(1, 2, 1, 1, "E"),
            pl(2, 0, 1, 1, "F"),
            pl(2, 1, 1, 1, "G"),
            pl(2, 2, 1, 1, ""),
            pl(3, 0, 1, 1, ""),
            pl(3, 1, 1, 1, "H"),
            pl(3, 2, 1, 1, ""),
            pl(4, 0, 1, 2, ""),
            pl(4, 2, 1, 1, "I"),
        ]
    );
    // The model holds them as cells, none made up by the layout.
    assert_eq!(made_up(t), 0);
    assert!(simplified(&im).is_empty());
}

#[test]
fn grid_after_widens_a_table_whose_cells_are_all_shorter() {
    let im = with_table(&[
        trp(r#"<w:gridAfter w:val="2"/>"#, &[cell("A"), cell("B")]),
        tr(&[cell("C"), cell("D")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.columns.len(), 4);
    assert_eq!(placed(t)[2], pl(0, 2, 1, 2, ""));
}

#[test]
fn a_vertical_merge_next_to_a_grid_before_stays_where_word_has_it() {
    // "A" is in the second column of the grid, the first has no cell.
    let im = with_table(&[
        trp(r#"<w:gridBefore w:val="1"/>"#, &[restart("A"), cell("x")]),
        trp(r#"<w:gridBefore w:val="1"/>"#, &[cont(), cell("y")]),
        tr(&[cell("p"), cell("q"), cell("r")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 1, ""),
            pl(0, 1, 2, 1, "A"),
            pl(0, 2, 1, 1, "x"),
            pl(1, 0, 1, 1, ""),
            pl(1, 2, 1, 1, "y"),
            pl(2, 0, 1, 1, "p"),
            pl(2, 1, 1, 1, "q"),
            pl(2, 2, 1, 1, "r"),
        ]
    );
}

#[test]
fn a_spanning_cell_keeps_its_own_alignment_when_it_differs_from_its_first_column() {
    let im = with_table(&[
        header_row(&[
            restart("Name"),
            tcp(r#"<w:gridSpan w:val="2"/>"#, &[pj("center", "Counts")]),
        ]),
        header_row(&[
            cont(),
            tc(&[pj("right", "Words")]),
            tc(&[pj("right", "Pages")]),
        ]),
        tr(&[cell("a"), tc(&[pj("right", "4")]), tc(&[pj("right", "5")])]),
        tr(&[
            cell("b"),
            tc(&[pj("right", "19")]),
            tc(&[pj("right", "20")]),
        ]),
        // A banner over the whole width, start-aligned: the first column is
        // left, so nothing to say.
        tr(&[span(3, "Part one")]),
        tr(&[tcp(
            r#"<w:gridSpan w:val="3"/>"#,
            &[pj("center", "Part two")],
        )]),
        // Over two right-aligned columns, aligned left, and right.
        tr(&[
            cell("c"),
            tcp(r#"<w:gridSpan w:val="2"/>"#, &[pj("left", "Both")]),
        ]),
        tr(&[
            cell("d"),
            tcp(r#"<w:gridSpan w:val="2"/>"#, &[pj("right", "Both")]),
        ]),
        // An empty spanning cell says nothing either.
        tr(&[
            cell("e"),
            tcp(r#"<w:gridSpan w:val="2"/>"#, &[pj("center", "")]),
        ]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    let aligns: Vec<_> = t.columns.iter().map(|c| c.align).collect();
    assert_eq!(
        aligns,
        [ColumnAlign::Left, ColumnAlign::Right, ColumnAlign::Right]
    );
    assert_eq!(t.header_rows, 2);
    let spanning: Vec<_> = t
        .rows
        .iter()
        .flatten()
        .filter(|c| c.colspan > 1)
        .map(|c| (Inline::plain_text(&c.content), c.align))
        .collect();
    assert_eq!(
        spanning,
        [
            ("Counts".to_string(), Some(ColumnAlign::Center)),
            ("Part one".to_string(), None),
            ("Part two".to_string(), Some(ColumnAlign::Center)),
            ("Both".to_string(), Some(ColumnAlign::Left)),
            ("Both".to_string(), None),
            (String::new(), None),
        ]
    );
    // Cells over one column never carry one: they follow their column.
    assert!(
        t.rows
            .iter()
            .flatten()
            .filter(|c| c.colspan == 1)
            .all(|c| c.align.is_none())
    );
}

#[test]
fn spanning_cells_do_not_vote_for_the_column() {
    // A centred banner over two columns does not make the first column
    // centred; the single cells decide.
    let im = with_table(&[
        tr(&[tcp(r#"<w:gridSpan w:val="2"/>"#, &[pj("center", "Banner")])]),
        tr(&[cell("a"), tc(&[pj("right", "4")])]),
        tr(&[cell("b"), tc(&[pj("right", "5")])]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    let aligns: Vec<_> = t.columns.iter().map(|c| c.align).collect();
    assert_eq!(aligns, [ColumnAlign::Left, ColumnAlign::Right]);
}

#[test]
fn a_table_of_one_merged_column_is_still_a_box() {
    // Every row is one cell across a two-column grid: one column to a
    // reader, so the cells stay paragraphs (TableFlattened).
    let im = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        format!(
            "<w:tbl>{}{}</w:tbl>",
            tr(&[span(2, "Notice")]),
            tr(&[span(2, "Midnight")])
        ),
        p("After."),
    ])
    .import();
    let blocks = &im.book.sections[0].blocks;
    assert!(
        !blocks.iter().any(|b| matches!(b, Block::Table(_))),
        "{blocks:?}"
    );
    assert!(kinds(&im).contains(&WarningKind::TableFlattened));
}

#[test]
fn a_tracked_change_to_cell_properties_is_not_read() {
    // The old properties inside `w:tcPrChange` / `w:trPrChange` are not the
    // cell's or the row's.
    let changed = tcp(
        r#"<w:tcPrChange w:id="1"><w:tcPr><w:gridSpan w:val="3"/><w:vMerge w:val="restart"/></w:tcPr></w:tcPrChange>"#,
        &[p("A")],
    );
    let im = with_table(&[
        trp(
            r#"<w:trPrChange w:id="2"><w:trPr><w:tblHeader/><w:gridBefore w:val="2"/></w:trPr></w:trPrChange>"#,
            &[changed, cell("B")],
        ),
        tr(&[cell("C"), cell("D")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(t.header_rows, 0);
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 1, 1, "A"),
            pl(0, 1, 1, 1, "B"),
            pl(1, 0, 1, 1, "C"),
            pl(1, 1, 1, 1, "D"),
        ]
    );
}

/// A list paragraph of the numbering in `numbering()`.
fn item(num_id: u32, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num_id}"/></w:numPr></w:pPr>{}</w:p>"#,
        run(text)
    )
}

/// Numbering 1: decimal; numbering 2: bullets.
fn numbering() -> String {
    format!(
        r#"<w:numbering {NS}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#
    )
}

#[test]
fn a_list_in_a_cell_is_lines_with_typed_markers() {
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        p("Before."),
        format!(
            "<w:tbl>{}{}</w:tbl>",
            tr(&[
                tc(&[
                    item(1, "first"),
                    item(1, "second"),
                    p("between"),
                    item(1, "again")
                ]),
                tc(&[item(2, "red"), item(2, "green")]),
            ]),
            tr(&[cell("x"), cell("y")]),
        ),
        p("After."),
    ]);
    d.numbering = Some(numbering());
    let im = d.import();
    let t = the_table(&im.book.sections[0].blocks[1]);
    let lines = |c: &TableCell| -> Vec<String> {
        c.content
            .split(|i| matches!(i, Inline::LineBreak))
            .map(Inline::plain_text)
            .collect()
    };
    assert_eq!(
        lines(&t.rows[0][0]),
        ["1. first", "2. second", "between", "1. again"]
    );
    assert_eq!(lines(&t.rows[0][1]), ["\u{2022} red", "\u{2022} green"]);
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("typed markers"), "{w:?}");
}

#[test]
fn rotated_text_in_a_cell_is_upright_and_said_once() {
    let rotated = |dir: &str, text: &str| {
        tcp(
            &format!(r#"<w:textDirection w:val="{dir}"/>"#),
            &[if text.is_empty() { empty() } else { p(text) }],
        )
    };
    let im = with_table(&[
        tr(&[rotated("btLr", "Station"), rotated("tbRl", "Times")]),
        tr(&[cell("Quay"), cell("12:30")]),
    ]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    assert_eq!(cell_texts(t), [["Station", "Times"], ["Quay", "12:30"]]);
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("rotated"), "{w:?}");

    // Left to right is no rotation, and an empty rotated cell has nothing
    // to turn.
    let im = with_table(&[
        tr(&[rotated("lrTb", "Station"), rotated("btLr", "")]),
        tr(&[cell("Quay"), cell("12:30")]),
    ]);
    assert!(simplified(&im).is_empty());
}

#[test]
fn nested_tables_become_lines_of_the_cell_even_when_deeper() {
    let inner = |rows: String| format!("<w:tbl>{rows}</w:tbl>");
    let deeper = inner(tr(&[tc(&[
        p("one"),
        inner(tr(&[cell("x"), cell("y")])),
        empty(),
    ])]));
    let nested = format!(
        "<w:tc>{}{}{}{}</w:tc>",
        p("Outer"),
        inner(tr(&[cell("in"), cell("side")]) + &tr(&[cell("c"), cell("d")])),
        deeper,
        empty()
    );
    let im = with_table(&[tr(&[cell("Left"), nested]), tr(&[cell("p"), cell("q")])]);
    let t = the_table(&im.book.sections[0].blocks[1]);
    let text = Inline::plain_text(&t.rows[0][1].content);
    for word in ["Outer", "in · side", "c · d", "one", "x", "y"] {
        assert!(text.contains(word), "{word}: {text:?}");
    }
    assert_eq!(t.rows.len(), 2);
    let w = simplified(&im);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].1.contains("inside a cell"), "{w:?}");
}

#[test]
fn a_table_in_a_footnote_ignores_spans() {
    // Notes hold blocks: a table there is joined by row, merged or not.
    let mut d = Docx::new(&[
        ps("Heading1", "Chapter 1"),
        format!(
            r#"<w:p>{}<w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#,
            run("Text")
        ),
    ]);
    d.footnotes = Some(format!(
        r#"<w:footnote w:id="1"><w:tbl>{}{}</w:tbl></w:footnote>"#,
        tr(&[restart("Quay"), cell("12:30")]),
        tr(&[cont(), cell("13:30")]),
    ));
    let im = d.import();
    assert_eq!(
        im.book.sections[0].blocks,
        vec![Block::paragraph(vec![
            Inline::text("Text"),
            Inline::Footnote {
                blocks: vec![para("Quay · 12:30"), para("– · 13:30")]
            }
        ])]
    );
}

/// The tables of a fixture file, in book order.
fn fixture_tables(file: &str) -> (Imported, Vec<Table>) {
    let bytes = std::fs::read(format!(
        "{}/tests/fixtures/docx/{file}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let im = import(&bytes).unwrap();
    let tables = im
        .book
        .sections
        .iter()
        .flat_map(|s| &s.blocks)
        .filter_map(|b| match b {
            Block::Table(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    (im, tables)
}

/// The lines of a cell (its text between line breaks).
fn cell_lines(c: &TableCell) -> Vec<String> {
    c.content
        .split(|i| matches!(i, Inline::LineBreak))
        .map(Inline::plain_text)
        .collect()
}

#[test]
fn a_table_saved_by_word_lays_out_as_word_draws_it() {
    // `word-en-tables.docx` was made by Word for Mac (`word-tables.
    // applescript`) and exported to PDF by Word; the grid below is what that
    // PDF shows, row by row: "Said" and "Words" over two header rows, "Lines"
    // centred over two columns, banner rows across all four, "Mr. Bennet"
    // over two rows, the numbers in the right-aligned last column, and the
    // short row ("Narrator") that ends one column early (`w:gridAfter`).
    let (im, tables) = fixture_tables("word-en-tables.docx");
    assert_eq!(tables.len(), 2, "the box of one cell stays paragraphs");
    let t = &tables[0];
    assert_eq!(t.header_rows, 2);
    assert_eq!(
        t.columns.iter().map(|c| c.align).collect::<Vec<_>>(),
        [
            ColumnAlign::Left,
            ColumnAlign::Left,
            ColumnAlign::Left,
            ColumnAlign::Right
        ]
    );
    let (lq, rq) = ('\u{201C}', '\u{201D}');
    assert_eq!(
        placed(t),
        [
            pl(0, 0, 2, 1, "Said"),
            pl(0, 1, 1, 2, "Lines"),
            pl(0, 3, 2, 1, "Words"),
            pl(1, 1, 1, 1, "Question"),
            pl(1, 2, 1, 1, "Answer"),
            pl(2, 0, 1, 4, "Chapter I"),
            pl(3, 0, 2, 1, "Mr. Bennet"),
            pl(3, 1, 1, 1, &format!("{lq}What is his name?{rq}")),
            pl(3, 2, 1, 1, &format!("{lq}Bingley.{rq}")),
            pl(3, 3, 1, 1, "5"),
            pl(4, 1, 1, 1, &format!("{lq}Is he married or single?{rq}")),
            pl(
                4,
                2,
                1,
                1,
                &format!("{lq}Oh! single, my dear, to be sure!{rq}")
            ),
            pl(4, 3, 1, 1, "12"),
            pl(5, 0, 1, 1, "Narrator"),
            pl(5, 1, 1, 1, "Mr. Bennet made no answer."),
            pl(5, 2, 1, 1, ""),
            pl(5, 3, 1, 1, ""),
            pl(6, 0, 1, 4, "Chapter II"),
            pl(7, 0, 1, 1, "Mr. Bennet"),
            pl(
                7,
                1,
                1,
                1,
                &format!("{lq}I hope Mr. Bingley will like it Lizzy.{rq}")
            ),
            pl(7, 2, 1, 1, ""),
            pl(7, 3, 1, 1, "7"),
        ]
    );
    // Centred over the two columns: its own alignment differs from the
    // first column's. The right-aligned "Words" is the column's.
    assert_eq!(t.rows[0][1].align, Some(ColumnAlign::Center));
    assert_eq!(t.rows[0][2].align, None);
    assert_eq!(t.rows[2][0].align, None);
    assert_eq!(t.rows[6][0].align, Some(ColumnAlign::Center));
    assert_eq!(made_up(t), 0);

    // The second table: what a cell can hold besides a line.
    let t = &tables[1];
    assert_eq!(t.header_rows, 1);
    assert_eq!(
        cell_lines(&t.rows[1][1]),
        [
            format!("{lq}Oh! single, my dear, to be sure!"),
            format!("A single man of large fortune; four or five thousand a year.{rq}")
        ]
    );
    assert_eq!(
        cell_lines(&t.rows[2][1]),
        [
            "1. Mr. Bennet replied that he had not.",
            "2. This was invitation enough.",
            "3. Mr. Bennet made no answer."
        ]
    );
    assert_eq!(t.rows[3][0].content, vec![Inline::text("Bingley.")]);
    assert_eq!(
        cell_lines(&t.rows[3][1]),
        [
            "Mr. Bennet replied that he had not.".to_string(),
            format!("{lq}What is his name?{rq} · {lq}Bingley.{rq}"),
            format!("{lq}Is he married or single?{rq} · {lq}Oh! single, my dear, to be sure!{rq}")
        ]
    );
    assert!(matches!(
        t.rows[4][1].content[..],
        [Inline::Text { .. }, Inline::Footnote { .. }]
    ));

    // One warning for the second table, in the words of what it lost.
    let w: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::TableSimplified)
        .collect();
    assert_eq!(w.len(), 1, "{w:?}");
    for part in [
        "several paragraphs",
        "typed markers",
        "inside a cell",
        "rotated",
    ] {
        assert!(w[0].message.contains(part), "{part}: {}", w[0].message);
    }
    assert_eq!(
        im.warnings
            .iter()
            .filter(|w| w.kind == WarningKind::TableFlattened)
            .count(),
        1
    );
}

#[test]
fn the_span_fixture_reads_every_merge_in_adr_0011e() {
    // `synthetic-en-table-spans.docx` (make-synthetic-tables.py).
    let (im, t) = fixture_tables("synthetic-en-table-spans.docx");
    assert_eq!(t.len(), 4, "the box of merged cells stays paragraphs");
    let (q1, a1) = ("What is his name?", "Bingley.");
    let (q2, a2) = (
        "Is he married or single?",
        "Oh! single, my dear, to be sure!",
    );

    // A label over two rows, a group heading over two columns, only the
    // first row marked: the header grows to two rows.
    assert_eq!(t[0].header_rows, 2);
    assert_eq!(
        placed(&t[0]),
        [
            pl(0, 0, 2, 1, "Said"),
            pl(0, 1, 1, 2, "Lines"),
            pl(1, 1, 1, 1, "Question"),
            pl(1, 2, 1, 1, "Answer"),
            pl(2, 0, 2, 1, "Mr. Bennet"),
            pl(2, 1, 1, 1, q1),
            pl(2, 2, 1, 1, a1),
            pl(3, 1, 1, 1, q2),
            pl(3, 2, 1, 1, a2),
        ]
    );
    assert_eq!(t[0].rows[0][1].align, Some(ColumnAlign::Center));

    // Continue cells with nothing to continue are ordinary cells.
    assert!(placed(&t[1]).iter().all(|c| c.2 == 1));
    assert_eq!(t[1].rows.len(), 6);
    assert_eq!(cell_texts(&t[1])[1], ["", q2, a2]);
    assert_eq!(cell_texts(&t[1])[3][1], "");
    assert_eq!((t[1].rows[5][0].colspan, t[1].rows[5][0].rowspan), (2, 1));

    // hMerge, gridBefore, gridAfter, a restart alone, a chain to the last
    // row with text left in its covered cell.
    let c = &t[2];
    assert_eq!(c.columns.len(), 4);
    assert_eq!(
        cell_lines(&c.rows[3][3]),
        ["Kitty", "The girls stared at their father."]
    );
    assert_eq!(
        placed(c)
            .iter()
            .map(|p| (p.0, p.1, p.2, p.3))
            .collect::<Vec<_>>(),
        [
            (0, 0, 1, 2),
            (0, 2, 1, 1),
            (0, 3, 1, 1),
            (1, 0, 1, 1),
            (1, 1, 1, 1),
            (1, 2, 1, 1),
            (1, 3, 1, 1),
            (2, 0, 1, 1),
            (2, 1, 1, 1),
            (2, 2, 1, 2),
            (3, 0, 1, 1),
            (3, 1, 1, 1),
            (3, 2, 1, 1),
            (3, 3, 2, 1),
            (4, 0, 1, 1),
            (4, 1, 1, 1),
            (4, 2, 1, 1),
        ]
    );
    assert_eq!(
        cell_texts(c)[..3],
        [
            vec!["Said", q1, a1],
            vec!["", q2, a2, "Mr. Bennet replied that he had not."],
            vec![
                "This was invitation enough.",
                "Mr. Bennet made no answer.",
                ""
            ],
        ]
    );
    assert_eq!(made_up(c), 0);

    // A cell wider than the others sets the width; the old properties of a
    // tracked change are not read.
    assert_eq!(t[3].columns.len(), 5);
    assert_eq!(t[3].rows[0][0].colspan, 5);
    assert_eq!(t[3].rows[1][0].colspan, 1);

    let simplified: Vec<_> = im
        .warnings
        .iter()
        .filter(|w| w.kind == WarningKind::TableSimplified)
        .map(|w| w.message.as_str())
        .collect();
    assert_eq!(simplified.len(), 2, "{simplified:?}");
    assert!(simplified[0].contains("nothing to merge into"));
    assert!(simplified[1].contains("covered cell"));
    assert_eq!(
        im.warnings
            .iter()
            .filter(|w| w.kind == WarningKind::TableFlattened)
            .count(),
        1
    );
}
