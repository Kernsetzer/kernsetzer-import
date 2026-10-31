#!/usr/bin/env python3 -I
"""Writes synthetic-en-tables.docx, synthetic-de-tables.docx and
synthetic-en-table-spans.docx.

Tables as Word writes them (`w:tbl`, `w:tblGrid`, `w:tblHeader`, `w:gridSpan`,
`w:vMerge`, `w:jc` in the cell paragraphs). Neither fixture has a producer:

- textutil cannot write them. `textutil -convert docx` drops every `<table>`
  of its HTML and writes the cells as paragraphs of their own (checked
  2026-10-30 with `<th>`, `colspan`, `rowspan`, and through RTF, which does
  hold the rows).
- Word is driven for the span fixture (`word-en-tables.docx`,
  `word-tables.applescript`). The shapes Word does not write (`w:hMerge`,
  `w:gridBefore`, a continue cell with nothing to continue, a span wider than
  the grid, text in a covered cell) are `synthetic-en-table-spans.docx`
  below.

So the XML is written by hand in the shape Word writes it. A table saved by
Word for Windows or Google Docs would be a welcome check of the shapes
against the real thing.

English: *Pride and Prejudice*, Jane Austen, chapters I and II
(`samples/en-sample.md`). A table of lines of dialogue (who says it, the line,
its number of words), one with a header row, a right-aligned numeric column,
an empty cell, a bold cell and a cell of two paragraphs; and a second table
that shows that paragraph properties inside a cell stay there: every cell
paragraph has an indent and a first-line indent and space before and after,
one a Quote style and one a Heading 2 style, and the table has a banner cell
over two columns (`gridSpan`) and a cell over two rows (`vMerge`), which
the importer reads as a `colspan` and a `rowspan`.

The span fixture (`synthetic-en-table-spans.docx`, Austen again, a few short
lines of chapters I to IV): one table each for a label over two rows and a
group heading over two columns with only the first row marked as a header
(the header grows), continue cells with nothing to continue (under a plain
cell, at another column, at another width) and a restart with no continue,
the legacy `w:hMerge`, `w:gridBefore`, `w:gridAfter`, a merge down to the
last row with text left in its covered cell, a `gridSpan` wider than the
grid and a `w:tcPrChange` that must not be read, and a box of one merged
column, which stays paragraphs.

German: Franz Kafka, *Ein Landarzt* (`samples/de-sample.md`). A table of
four stories (title, first words, number of words in them, number of
paragraphs in the story), one column centred, an empty paragraph before and
after the table (the author's habit and Word's); and a boxed one-column
note, which stays paragraphs.

The text is verbatim from the samples (sources in `samples/README.md`), with
the typographic quotes Word would write; the script checks that every line
occurs there. The word counts are the cells' own.

Run from this directory: `python3 -I make-synthetic-tables.py`, then
re-bless the goldens (`SETZER_BLESS=1 cargo test -p setzer-import --test
golden`) and review the diff. Stdlib only.
"""

import zipfile
from pathlib import Path

NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
SAMPLES = Path(__file__).resolve().parents[5] / "samples"


def styles(lang):
    return (
        f"<w:styles {NS}>"
        f'<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="{lang}"/></w:rPr></w:rPrDefault></w:docDefaults>'
        '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/>'
        '<w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/>'
        '<w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>'
        '<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/>'
        "<w:rPr><w:i/></w:rPr></w:style>"
        '<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/></w:style>'
        "</w:styles>"
    )


def core(title, author, lang):
    return (
        '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/">'
        f"<dc:title>{title}</dc:title><dc:creator>{author}</dc:creator>"
        f"<dc:language>{lang}</dc:language></cp:coreProperties>"
    )


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def para(text, style=None, jc=None, bold=False, ind=None, spacing=None):
    """A paragraph; `text` None is an empty one."""
    ppr = ""
    if style:
        ppr += f'<w:pStyle w:val="{style}"/>'
    if spacing:
        ppr += spacing
    if ind:
        ppr += ind
    if jc:
        ppr += f'<w:jc w:val="{jc}"/>'
    ppr = f"<w:pPr>{ppr}</w:pPr>" if ppr else ""
    if text is None:
        return f"<w:p>{ppr}</w:p>"
    rpr = "<w:rPr><w:b/><w:bCs/></w:rPr>" if bold else ""
    return f'<w:p>{ppr}<w:r>{rpr}<w:t xml:space="preserve">{esc(text)}</w:t></w:r></w:p>'


def cell(paras, width, span=1, vmerge=None, hmerge=None, extra=""):
    """A cell holding the given paragraph XML; `vmerge` and `hmerge` are
    "restart" or "continue"; `extra` is more `w:tcPr` XML."""
    pr = f'<w:tcW w:w="{width}" w:type="dxa"/>'
    if span > 1:
        pr += f'<w:gridSpan w:val="{span}"/>'
    if hmerge == "restart":
        pr += '<w:hMerge w:val="restart"/>'
    elif hmerge == "continue":
        pr += "<w:hMerge/>"
    if vmerge == "restart":
        pr += '<w:vMerge w:val="restart"/>'
    elif vmerge == "continue":
        pr += "<w:vMerge/>"
    pr += extra
    return f"<w:tc><w:tcPr>{pr}</w:tcPr>{''.join(paras)}</w:tc>"


def table(widths, rows):
    """`rows`: a list of (header, [cell XML]) pairs, or (header, [cell XML],
    `w:trPr` XML) triples (`w:gridBefore`, `w:gridAfter`)."""
    x = (
        '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/>'
        f'<w:tblW w:w="{sum(widths)}" w:type="dxa"/>'
        '<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" '
        'w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>'
        + "".join(f'<w:gridCol w:w="{w}"/>' for w in widths)
        + "</w:tblGrid>"
    )
    for header, cells, *more in rows:
        trpr = ("<w:tblHeader/>" if header else "") + "".join(more)
        trpr = f"<w:trPr>{trpr}</w:trPr>" if trpr else ""
        x += f"<w:tr>{trpr}{''.join(cells)}</w:tr>"
    return x + "</w:tbl>"


def words(s):
    return str(len(s.split()))


def write(name, lang, title, author, body, sources):
    sample, lines = sources
    text = (SAMPLES / sample).read_text()
    flat = text.replace("“", '"').replace("”", '"').replace("’", "'").replace("‘", "'")
    flat = flat.replace(" -- ", " -- ").replace("_", "")
    for line in lines:
        plain = line.replace("“", '"').replace("”", '"').replace("’", "'")
        assert plain in flat, f"{line!r} is not in {sample}"
    document = f"<w:document {NS}><w:body>{''.join(body)}</w:body></w:document>"
    parts = [
        ("[Content_Types].xml", "<Types/>"),
        (
            "_rels/.rels",
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/>'
            '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>',
        ),
        ("word/document.xml", document),
        (
            "word/_rels/document.xml.rels",
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rS" Type="{REL}/styles" Target="styles.xml"/></Relationships>',
        ),
        ("word/styles.xml", styles(lang)),
        ("docProps/core.xml", core(title, author, lang)),
    ]
    with zipfile.ZipFile(name, "w", zipfile.ZIP_DEFLATED) as z:
        for part, data in parts:
            info = zipfile.ZipInfo(part, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data.encode())


# ------------------------------------------------------------------ English

W3 = [1700, 5500, 1300]
LINES_EN = [
    ("Mr. Bennet", "“What is his name?”"),
    ("Mrs. Bennet", "“Bingley.”"),
    ("Mr. Bennet", "“Is he married or single?”"),
]
SIGH = "Mr. Bennet made no answer."
OH = "“Oh! single, my dear, to be sure!"
YEAR = "A single man of large fortune; four or five thousand a year."


def dialogue_table():
    rows = [
        (
            True,
            [
                cell([para("Said", bold=True)], W3[0]),
                cell([para("Line", bold=True)], W3[1]),
                cell([para("Words", bold=True, jc="right")], W3[2]),
            ],
        )
    ]
    for n, (who, line) in enumerate(LINES_EN):
        rows.append(
            (
                False,
                [
                    cell([para(who)], W3[0]),
                    # A bold cell among plain ones.
                    cell([para(line, bold=n == 1)], W3[1]),
                    cell([para(words(line), jc="right")], W3[2]),
                ],
            )
        )
    # An empty first cell.
    rows.append(
        (
            False,
            [
                cell([para(None)], W3[0]),
                cell([para(SIGH)], W3[1]),
                cell([para(words(SIGH), jc="right")], W3[2]),
            ],
        )
    )
    # A cell of two paragraphs.
    rows.append(
        (
            False,
            [
                cell([para("Mrs. Bennet")], W3[0]),
                cell([para(OH), para(YEAR + "”")], W3[1]),
                cell([para(words(OH + " " + YEAR), jc="right")], W3[2]),
            ],
        )
    )
    return table(W3, rows)


# Paragraph properties that must stay inside the cell.
LEAK = dict(
    ind='<w:ind w:left="720" w:right="720" w:firstLine="360"/>',
    spacing='<w:spacing w:before="240" w:after="240"/>',
)
W2 = [2000, 7000]


def indented_table():
    def line(who, text, **kw):
        return para(text, **{**LEAK, **kw})

    rows = [
        # A banner over both columns.
        (False, [cell([line(None, "Chapter II", style="Heading2")], sum(W2), span=2)]),
        (
            False,
            [
                cell([line(None, "Mr. Bennet", style="Heading2")], W2[0], vmerge="restart"),
                cell(
                    [line(None, "“I hope Mr. Bingley will like it Lizzy.”", style="Quote")],
                    W2[1],
                ),
            ],
        ),
        (
            False,
            [
                cell([line(None, None)], W2[0], vmerge="continue"),
                cell(
                    [
                        line(
                            None,
                            "“No more have I,” said Mr. Bennet; “and I am glad to find that you do not depend on her serving you.”",
                        )
                    ],
                    W2[1],
                ),
            ],
        ),
        (
            False,
            [
                cell([line(None, "Mrs. Bennet")], W2[0]),
                cell([line(None, "“Don’t keep coughing so, Kitty, for heaven’s sake!”")], W2[1]),
            ],
        ),
    ]
    return table(W2, rows)


en_lines = [
    "Mr. Bennet replied that he had not.",
    "This was invitation enough.",
    "Mrs. Bennet deigned not to make any reply; but unable to contain herself, began scolding one of her daughters.",
    "“No more have I,” said Mr. Bennet; “and I am glad to find that you do not depend on her serving you.”",
    "“Don’t keep coughing so, Kitty, for heaven’s sake!",
    "“I hope Mr. Bingley will like it Lizzy.”",
    "What is his name?",
    "Bingley.",
    "Is he married or single?",
    SIGH,
    OH,
    YEAR,
]
write(
    "synthetic-en-tables.docx",
    "en-GB",
    "Pride and Prejudice",
    "Jane Austen",
    [
        para("Chapter I", "Heading1"),
        para("Mr. Bennet replied that he had not."),
        dialogue_table(),
        para(None),
        para("This was invitation enough."),
        para("Chapter II", "Heading1"),
        para(
            "Mrs. Bennet deigned not to make any reply; but unable to contain herself, began scolding one of her daughters."
        ),
        indented_table(),
        para(None),
        para("This was invitation enough."),
    ],
    ("en-sample.md", en_lines),
)

# ------------------------------------------------------------------- German

W4 = [2300, 4200, 1200, 1300]
STORIES = [
    ("Der neue Advokat", "Wir haben einen neuen Advokaten, den Dr. Bucephalus.", "3"),
    ("Ein Landarzt", "Ich war in großer Verlegenheit:", "5"),
    ("Vor dem Gesetz", "Vor dem Gesetz steht ein Türhüter.", "1"),
    ("Das nächste Dorf", "Mein Großvater pflegte zu sagen:", "1"),
]


def stories_table():
    rows = [
        (
            True,
            [
                cell([para("Titel", bold=True)], W4[0]),
                cell([para("Anfang", bold=True)], W4[1]),
                cell([para("Wörter", bold=True, jc="right")], W4[2]),
                cell([para("Absätze", bold=True, jc="center")], W4[3]),
            ],
        )
    ]
    for title, start, paras in STORIES:
        rows.append(
            (
                False,
                [
                    cell([para(title)], W4[0]),
                    cell([para(start)], W4[1]),
                    cell([para(words(start), jc="right")], W4[2]),
                    cell([para(paras, jc="center")], W4[3]),
                ],
            )
        )
    return table(W4, rows)


NOTE = "Vor dem Gesetz steht ein Türhüter."
de_lines = [s for _, s, _ in STORIES] + [
    "Im allgemeinen billigt das Barreau die Aufnahme des Bucephalus.",
    NOTE,
    "Zu diesem Türhüter kommt ein Mann vom Lande und bittet um Eintritt in das Gesetz.",
]
write(
    "synthetic-de-tables.docx",
    "de-DE",
    "Ein Landarzt",
    "Franz Kafka",
    [
        para("Der neue Advokat", "Heading1"),
        para("Im allgemeinen billigt das Barreau die Aufnahme des Bucephalus."),
        para(None),
        stories_table(),
        para(None),
        para("Wir haben einen neuen Advokaten, den Dr. Bucephalus."),
        para("Vor dem Gesetz", "Heading1"),
        table([9000], [(False, [cell([para(NOTE)], 9000)])]),
        para("Zu diesem Türhüter kommt ein Mann vom Lande und bittet um Eintritt in das Gesetz."),
    ],
    ("de-sample.md", de_lines),
)

# ---------------------------------------------------- English, merged cells

# Every shape of merge the importer reads, one table each, in Word's XML.
Q1, A1 = "What is his name?", "Bingley."
Q2, A2 = "Is he married or single?", "Oh! single, my dear, to be sure!"
N1 = "Mr. Bennet replied that he had not."
N2 = "This was invitation enough."
N3 = "Mr. Bennet made no answer."
N4 = "The girls stared at their father."
N5 = "Nonsense, nonsense!"
S1 = "I am sick of Mr. Bingley"
S2 = "let us return to Mr. Bingley"
S3 = "What an excellent father you have, girls"
W3S = [2200, 3300, 3300]


def c(text, width, **kw):
    """A cell of one paragraph (empty when `text` is None)."""
    jc = kw.pop("jc", None)
    return cell([para(text, jc=jc)], width, **kw)


def spans_header_growth():
    """A label over two rows and a group heading over two columns; only the
    first row is marked as a header, so the span grows the header to two
    rows."""
    a, b, d = W3S
    return table(
        W3S,
        [
            (True, [c("Said", a, vmerge="restart"), c("Lines", b + d, span=2, jc="center")]),
            (False, [c(None, a, vmerge="continue"), c("Question", b), c("Answer", d)]),
            (False, [c("Mr. Bennet", a, vmerge="restart"), c(Q1, b), c(A1, d)]),
            (False, [c(None, a, vmerge="continue"), c(Q2, b), c(A2, d)]),
        ],
    )


def spans_orphans():
    """Cells that continue nothing: no restart above, at another column,
    at another width. A restart with no continue is an ordinary cell."""
    a, b, d = W3S
    return table(
        W3S,
        [
            (False, [c("Mr. Bennet", a), c(Q1, b), c(A1, d)]),
            # A continue cell under a plain cell.
            (False, [c(None, a, vmerge="continue"), c(Q2, b), c(A2, d)]),
            (False, [c("Mrs. Bennet", a, vmerge="restart"), c(N5, b), c(S1, d)]),
            # A continue cell in another column than the restart above.
            (False, [c("Girls", a), c(None, b, vmerge="continue"), c(N4, d)]),
            (False, [c("Kitty", a, vmerge="restart"), c(S2, b), c(S3, d)]),
            # A continue cell as wide as two columns, over a one-column restart.
            (False, [c(None, a + b, span=2, vmerge="continue"), c(N3, d)]),
        ],
    )


def spans_legacy_and_short_rows():
    """hMerge, gridBefore, gridAfter, a restart with nothing below it, a
    merge down to the last row, and text left in a covered cell."""
    w = [1800, 2600, 2600, 1800]
    return table(
        w,
        [
            (
                False,
                [
                    c("Said", w[0] + w[1], hmerge="restart"),
                    c(None, w[1], hmerge="continue"),
                    c(Q1, w[2]),
                    c(A1, w[3]),
                ],
            ),
            (False, [c(Q2, w[1]), c(A2, w[2]), c(N1, w[3])], '<w:gridBefore w:val="1"/>'),
            (False, [c(N2, w[0]), c(N3, w[1])], '<w:gridAfter w:val="2"/>'),
            (
                False,
                [
                    c("Mrs. Bennet", w[0], vmerge="restart"),
                    c(N5, w[1]),
                    c(S1, w[2]),
                    c("Kitty", w[3], vmerge="restart"),
                ],
            ),
            (
                False,
                [
                    c("Mr. Bennet", w[0]),
                    c(S2, w[1]),
                    c(S3, w[2]),
                    # Word leaves it empty; this one has text.
                    c(N4, w[3], vmerge="continue"),
                ],
            ),
        ],
    )


def spans_wide_and_changed():
    """A cell wider than the grid, and the old properties of a tracked
    change to a cell, which are not the cell's."""
    a, b, d = W3S
    old = '<w:tcPrChange w:id="1" w:author="A" w:date="2026-01-01T00:00:00Z"><w:tcPr><w:gridSpan w:val="3"/></w:tcPr></w:tcPrChange>'
    return table(
        W3S,
        [
            (False, [c(N5, sum(W3S) + 2 * a, span=5)]),
            (False, [c(N1, a, extra=old), c(Q1, b), c(A1, d)]),
        ],
    )


def spans_box():
    """Every row one cell across two columns: a box, so paragraphs."""
    return table(
        [4000, 4000],
        [
            (False, [c(N2, 8000, span=2)]),
            (False, [c(N3, 8000, span=2)]),
        ],
    )


span_lines = [N1, N2, N3, N4, N5, Q1, A1, Q2, A2, S1, S2, S3, "Bingley."]
write(
    "synthetic-en-table-spans.docx",
    "en-GB",
    "Pride and Prejudice",
    "Jane Austen",
    [
        para("Chapter I", "Heading1"),
        para(N1),
        spans_header_growth(),
        para(None),
        para(N2),
        para("Chapter II", "Heading1"),
        para(N3),
        spans_orphans(),
        para(None),
        para(N4),
        para("Chapter III", "Heading1"),
        para(N5),
        spans_legacy_and_short_rows(),
        para(None),
        para(N1),
        para("Chapter IV", "Heading1"),
        para(N2),
        spans_wide_and_changed(),
        para(None),
        para(N3),
        spans_box(),
        para(N4),
    ],
    ("en-sample.md", span_lines),
)
