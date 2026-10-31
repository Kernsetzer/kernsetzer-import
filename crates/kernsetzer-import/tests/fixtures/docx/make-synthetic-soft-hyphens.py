#!/usr/bin/env python3 -I
"""Writes synthetic-de-soft-hyphens.docx: an author's hand-placed hyphens.

Word's manual hyphenation (and Ctrl+-) writes an optional hyphen as
`<w:softHyphen/>` inside a run; text pasted from elsewhere carries U+00AD in
`w:t`. Both must reach the model as U+00AD. Cases: `w:softHyphen` between
two `w:t` of one run, U+00AD in the text, and a `w:softHyphen` in a run of
its own between two italic runs.

Run from this directory: `python3 -I make-synthetic-soft-hyphens.py`, then
re-bless the goldens (`SETZER_BLESS=1 cargo test -p setzer-import --test
golden`) and review the diff. Stdlib only. textutil cannot write
`w:softHyphen`, and Word is not driven for fixtures, so the XML is written by
hand in the shape Word writes. The text is Kafka's *Ein Landarzt* (public
domain, `samples/de-sample.md`).
"""

import zipfile

NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
SHY = "<w:softHyphen/>"

STYLES = (
    f"<w:styles {NS}>"
    '<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="de-DE"/></w:rPr></w:rPrDefault></w:docDefaults>'
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
    '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/>'
    '<w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>'
    "</w:styles>"
)


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def t(s):
    return f'<w:t xml:space="preserve">{esc(s)}</w:t>'


def run(*parts, italic=False):
    rpr = "<w:rPr><w:i/></w:rPr>" if italic else ""
    return f"<w:r>{rpr}{''.join(parts)}</w:r>"


def para(*runs, style=None):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f"<w:p>{ppr}{''.join(runs)}</w:p>"


paras = [
    para(run(t("Ein Landarzt")), style="Heading1"),
    # Word: w:softHyphen between the w:t of one run.
    para(
        run(
            t("Ich war in großer Ver"),
            SHY,
            t("legenheit: eine dringende Reise stand mir bevor; ein Schwer"),
            SHY,
            t("kranker wartete auf mich in einem zehn Meilen entfernten Dorfe; starkes Schnee"),
            SHY,
            t("gestöber füllte den weiten Raum zwischen mir und ihm;"),
        )
    ),
    # U+00AD typed or pasted into the text.
    para(
        run(
            t(
                "in den Pelz gepackt, die Instrumenten­tasche in der Hand, stand ich "
                "reise­fertig schon auf dem Hofe; aber das Pferd fehlte, das Pferd."
            )
        )
    ),
    # A soft hyphen in a run of its own, inside an italic word.
    para(
        run(t("Mein eigenes Pferd war in der letzten Nacht, infolge der ")),
        run(t("Über"), italic=True),
        run(SHY, italic=True),
        run(t("anstrengung"), italic=True),
        run(t(" in diesem eisigen Winter, verendet.")),
    ),
]

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
    ("word/styles.xml", STYLES),
]
with zipfile.ZipFile("synthetic-de-soft-hyphens.docx", "w", zipfile.ZIP_DEFLATED) as z:
    for name, data in parts:
        info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(info, data.encode())
