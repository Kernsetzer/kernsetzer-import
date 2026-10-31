#!/usr/bin/env python3 -I
"""Writes synthetic-en-bare-parts.docx and synthetic-de-bare-parts.docx:
bare part labels ("Part Two", "Teil 2") straight above a chapter whose
heading is only a title.

The Word template tells authors to put parts in Heading 1 and
chapters in Heading 2. A heading-styled line under a bare label used to
become the label's title line, so "Part Two" over "The Letter" imported as
one part, "Part Two: The Letter", and the chapter was lost.

- `synthetic-en-bare-parts`: parts in Heading 1, chapters in Heading 2,
  and a third part with a real title line in Heading 1 ("Netherfield")
  over its first chapter, which stays the part's title.
- `synthetic-de-bare-parts`: everything in Heading 1, as authors without
  a template often do: "Teil 1" over "Der neue Advokat".

Run from this directory: `python3 -I make-synthetic-bare-parts.py`, then
re-bless the goldens (`SETZER_BLESS=1 cargo test -p setzer-import --test
golden`) and review the diff. Stdlib only; the XML is written by hand in
the shape Word writes. The chapter text is the first paragraph under a
heading of `samples/en-sample.md` (Austen) and `samples/de-sample.md`
(Kafka), read verbatim from there; the chapter titles of the German file
are Kafka's own.
"""

import pathlib
import zipfile

NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
SAMPLES = pathlib.Path(__file__).resolve().parents[5] / "samples"


def styles(lang):
    return (
        f"<w:styles {NS}>"
        f'<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="{lang}"/></w:rPr></w:rPrDefault></w:docDefaults>'
        '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/>'
        '<w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/>'
        '<w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>'
        "</w:styles>"
    )


def first_paragraph(sample, heading):
    """The first paragraph under `# heading` in a sample manuscript."""
    lines = (SAMPLES / sample).read_text(encoding="utf-8").splitlines()
    i = next(k for k, l in enumerate(lines) if l.split(" {")[0] == f"# {heading}")
    return next(l for l in lines[i + 1 :] if l.strip())


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def para(text, style=None):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{esc(text)}</w:t></w:r></w:p>'


def write(name, lang, paras):
    document = f"<w:document {NS}><w:body>{''.join(paras)}</w:body></w:document>"
    parts = [
        ("[Content_Types].xml", "<Types/>"),
        (
            "_rels/.rels",
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>',
        ),
        ("word/document.xml", document),
        (
            "word/_rels/document.xml.rels",
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rS" Type="{REL}/styles" Target="styles.xml"/></Relationships>',
        ),
        ("word/styles.xml", styles(lang)),
    ]
    with zipfile.ZipFile(name, "w", zipfile.ZIP_DEFLATED) as z:
        for part, data in parts:
            info = zipfile.ZipInfo(part, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data.encode())


en = lambda h: para(first_paragraph("en-sample.md", h))
write(
    "synthetic-en-bare-parts.docx",
    "en-GB",
    [
        para("Part One", "Heading1"),
        para("The Ball", "Heading2"),
        en("Chapter I"),
        para("Part Two", "Heading1"),
        para("The Letter", "Heading2"),
        en("Chapter II"),
        para("Part Three", "Heading1"),
        para("Netherfield", "Heading1"),
        para("The Visit", "Heading2"),
        en("Chapter III"),
    ],
)

de = lambda h: para(first_paragraph("de-sample.md", h))
write(
    "synthetic-de-bare-parts.docx",
    "de-DE",
    [
        para("Teil 1", "Heading1"),
        para("Der neue Advokat", "Heading1"),
        de("Der neue Advokat"),
        para("Teil 2", "Heading1"),
        para("Ein Landarzt", "Heading1"),
        de("Ein Landarzt"),
        para("Teil 3", "Heading1"),
        para("Auf der Galerie", "Heading1"),
        de("Auf der Galerie"),
    ],
)
