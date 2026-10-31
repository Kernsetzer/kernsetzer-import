//! Scrambling the text of an XML part without touching anything else.
//!
//! The part is read as events only to find where text is; the copy is the
//! original string with those stretches replaced, so every tag, attribute
//! order, quote, namespace declaration and space stays byte for byte. Text
//! is scrambled a paragraph at a time: a word that a producer split across
//! runs (Pages splits runs at every non-ASCII char) is one word again.
//!
//! A part that is not well-formed is an error. It is not repaired.

use quick_xml::Reader;
use quick_xml::events::Event;

use super::words::Scrambler;

/// Which elements and attributes of a part hold prose. Names are local
/// names: producers agree on namespaces but not on prefixes.
pub(super) struct Rules {
    /// Elements whose content is text (`w:t`).
    pub text: &'static [&'static str],
    /// Elements that group text into one unit (`w:p`): a word is not split
    /// by the end of a run, but by a line break or a field.
    pub paragraph: &'static [&'static str],
    /// Elements that stand for a char between text nodes of a paragraph: a
    /// line break or tab ends a word (`'\n'`), a soft hyphen does not.
    pub breaks: &'static [(&'static str, char)],
    /// `(element, attribute)` pairs whose value is prose (`"*"`: any
    /// element): an author's name, a picture's alt text.
    pub attrs: &'static [(&'static str, &'static str)],
    /// Scramble every text node, comment and CDATA section, not only the
    /// elements of `text`: for the parts whose elements the rules cannot
    /// know (a main document that is not under `word/`, `customXml/`,
    /// `docProps/custom.xml`).
    pub all_text: bool,
    /// With `all_text`: elements whose text is code or data, not prose (a
    /// field's instruction, a date), and stays.
    pub keep: &'static [&'static str],
}

/// One decoded char of a text node; `raw` is the entity it came from, put
/// back when the char is not changed.
struct Tok {
    c: char,
    raw: Option<String>,
}

/// The scrambled form of `xml`.
pub(super) fn scrub(xml: &str, rules: &Rules, sc: &Scrambler) -> Result<String, String> {
    let mut run = Run {
        xml,
        rules,
        sc,
        edits: Vec::new(),
        units: Vec::new(),
    };
    let mut reader = Reader::from_str(xml);
    let mut open_text: Option<(String, usize)> = None;
    // With `all_text`: where the last tag ended, and the open elements.
    let mut last_end = 0;
    let mut stack: Vec<String> = Vec::new();
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader
            .read_event()
            .map_err(|e| format!("{e} (at byte {before})"))?;
        let after = reader.buffer_position() as usize;
        if matches!(event, Event::Text(_) | Event::GeneralRef(_)) {
            continue;
        }
        if rules.all_text && open_text.is_none() {
            let parent = stack.last().map(String::as_str);
            if !parent.is_some_and(|p| rules.keep.contains(&p)) {
                run.stretch(last_end, before)?;
                match event {
                    Event::CData(_) if after >= before + 12 => {
                        run.raw_text((before + 9, after - 3));
                    }
                    Event::Comment(_) if after >= before + 7 => {
                        run.raw_text((before + 4, after - 3));
                    }
                    _ => {}
                }
            }
        }
        last_end = after;
        match event {
            Event::Start(e) => {
                let name = local(e.name().as_ref());
                if rules.all_text {
                    stack.push(name.clone());
                }
                run.attributes(&name, before, after)?;
                if open_text.is_some() {
                    continue;
                }
                if rules.paragraph.contains(&name.as_str()) {
                    run.units.push(Unit::default());
                } else if rules.text.contains(&name.as_str()) {
                    open_text = Some((name, after));
                } else if let Some(&(_, c)) = rules.breaks.iter().find(|(n, _)| *n == name) {
                    run.mark(c);
                }
            }
            Event::Empty(e) => {
                let name = local(e.name().as_ref());
                run.attributes(&name, before, after)?;
                if open_text.is_none()
                    && let Some(&(_, c)) = rules.breaks.iter().find(|(n, _)| *n == name)
                {
                    run.mark(c);
                }
            }
            Event::End(e) => {
                let name = local(e.name().as_ref());
                stack.pop();
                if let Some((open, start)) = &open_text {
                    if *open == name {
                        let range = (*start, before);
                        open_text = None;
                        run.text(range)?;
                    }
                } else if rules.paragraph.contains(&name.as_str()) {
                    run.end_unit()?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if open_text.is_some() || !run.units.is_empty() {
        return Err("the part ends inside an element".into());
    }
    Ok(run.apply())
}

fn local(qname: &str) -> String {
    qname.rsplit(':').next().unwrap_or(qname).to_string()
}

/// A text node of the paragraph being read.
struct Node {
    range: (usize, usize),
    toks: Vec<Tok>,
}

/// What a paragraph holds, in order: chars of text nodes, and the chars
/// that stand for elements between them.
#[derive(Default)]
struct Unit {
    nodes: Vec<Node>,
    items: Vec<Item>,
}

impl Unit {
    fn add(&mut self, range: (usize, usize), toks: Vec<Tok>) {
        let node = self.nodes.len();
        self.items
            .extend((0..toks.len()).map(|tok| Item::Text { node, tok }));
        self.nodes.push(Node { range, toks });
    }
}

enum Item {
    Text { node: usize, tok: usize },
    Mark(char),
}

struct Run<'a> {
    xml: &'a str,
    rules: &'a Rules,
    sc: &'a Scrambler,
    /// `(start, end, replacement)` in byte offsets of `xml`, in no order.
    edits: Vec<(usize, usize, String)>,
    units: Vec<Unit>,
}

impl Run<'_> {
    fn mark(&mut self, c: char) {
        if let Some(u) = self.units.last_mut() {
            u.items.push(Item::Mark(c));
        }
    }

    /// A text node ends: it joins the open paragraph, or is its own.
    fn text(&mut self, range: (usize, usize)) -> Result<(), String> {
        let toks = decode(&self.xml[range.0..range.1])?;
        if toks.is_empty() {
            return Ok(());
        }
        match self.units.last_mut() {
            Some(unit) => unit.add(range, toks),
            None => {
                let mut unit = Unit::default();
                unit.add(range, toks);
                self.finish(unit);
            }
        }
        Ok(())
    }

    /// The stretch of `xml[from..to]` between two tags, if it holds more
    /// than the whitespace a producer indents with.
    fn stretch(&mut self, from: usize, to: usize) -> Result<(), String> {
        let blank = self.xml[from..to]
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'));
        if blank {
            return Ok(());
        }
        self.text((from, to))
    }

    /// A comment's or a CDATA section's content: no entities, so a `&` or `<`
    /// in it is a char of its own, and it is never part of a paragraph.
    fn raw_text(&mut self, range: (usize, usize)) {
        let toks = self.xml[range.0..range.1]
            .chars()
            .map(|c| Tok { c, raw: None })
            .collect();
        let mut unit = Unit::default();
        unit.add(range, toks);
        self.finish(unit);
    }

    fn end_unit(&mut self) -> Result<(), String> {
        let unit = self.units.pop().ok_or("an end tag without its start")?;
        self.finish(unit);
        Ok(())
    }

    /// Scramble a paragraph and record what its text nodes become.
    fn finish(&mut self, mut unit: Unit) {
        let chars: Vec<char> = unit
            .items
            .iter()
            .map(|it| match *it {
                Item::Text { node, tok } => unit.nodes[node].toks[tok].c,
                Item::Mark(c) => c,
            })
            .collect();
        let protect = vec![false; chars.len()];
        let new = self.sc.scramble(&chars, &protect, true);
        let mut changed = vec![false; unit.nodes.len()];
        for (it, &c) in unit.items.iter().zip(&new) {
            if let Item::Text { node, tok } = *it {
                let t = &mut unit.nodes[node].toks[tok];
                if t.c != c {
                    t.c = c;
                    t.raw = None;
                    changed[node] = true;
                }
            }
        }
        for (node, changed) in unit.nodes.iter().zip(changed) {
            if changed {
                let text: String = node
                    .toks
                    .iter()
                    .map(|t| t.raw.clone().unwrap_or_else(|| t.c.to_string()))
                    .collect();
                self.edits.push((node.range.0, node.range.1, text));
            }
        }
    }

    /// Scramble the attributes of the start tag `xml[start..end]` the rules
    /// name.
    fn attributes(&mut self, element: &str, start: usize, end: usize) -> Result<(), String> {
        let wanted: Vec<&str> = self
            .rules
            .attrs
            .iter()
            .filter(|(e, _)| *e == "*" || *e == element)
            .map(|(_, a)| *a)
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let tag = &self.xml[start..end];
        for (name, v0, v1) in attribute_spans(tag) {
            if !wanted.contains(&name.as_str()) {
                continue;
            }
            let mut toks = decode(&tag[v0..v1])?;
            let chars: Vec<char> = toks.iter().map(|t| t.c).collect();
            let protect = vec![false; chars.len()];
            let new = self.sc.scramble(&chars, &protect, false);
            let mut changed = false;
            for (t, &c) in toks.iter_mut().zip(&new) {
                if t.c != c {
                    t.c = c;
                    t.raw = None;
                    changed = true;
                }
            }
            if changed {
                let text: String = toks
                    .iter()
                    .map(|t| t.raw.clone().unwrap_or_else(|| t.c.to_string()))
                    .collect();
                self.edits.push((start + v0, start + v1, text));
            }
        }
        Ok(())
    }

    fn apply(mut self) -> String {
        self.edits.sort_by_key(|e| e.0);
        let mut out = String::with_capacity(self.xml.len());
        let mut at = 0;
        for (s, e, text) in &self.edits {
            out.push_str(&self.xml[at..*s]);
            out.push_str(text);
            at = *e;
        }
        out.push_str(&self.xml[at..]);
        out
    }
}

/// `(local name, value start, value end)` of each attribute of a start
/// tag, as byte offsets into the tag.
fn attribute_spans(tag: &str) -> Vec<(String, usize, usize)> {
    let b = tag.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    let ws = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    // The element name.
    while i < b.len() && !ws(b[i]) && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    loop {
        while i < b.len() && ws(b[i]) {
            i += 1;
        }
        if i >= b.len() || matches!(b[i], b'/' | b'>') {
            break;
        }
        let n0 = i;
        while i < b.len() && !ws(b[i]) && !matches!(b[i], b'=' | b'/' | b'>') {
            i += 1;
        }
        let name = &tag[n0..i];
        while i < b.len() && ws(b[i]) {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while i < b.len() && ws(b[i]) {
            i += 1;
        }
        let Some(&q) = b.get(i).filter(|q| matches!(q, b'"' | b'\'')) else {
            break;
        };
        let v0 = i + 1;
        let Some(len) = b[v0..].iter().position(|&c| c == q) else {
            break;
        };
        out.push((
            name.rsplit(':').next().unwrap_or(name).to_string(),
            v0,
            v0 + len,
        ));
        i = v0 + len + 1;
    }
    out
}

/// The chars of a text node or attribute value, entities decoded. A
/// reference that is not one of XML's own or a char number is kept as it
/// is, as one char that is no letter.
fn decode(raw: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '<' => return Err("markup inside text (CDATA or a comment)".into()),
            '&' => {
                let rest = &raw[i + 1..];
                let end = rest
                    .find(';')
                    .filter(|&e| e <= 12)
                    .ok_or("an '&' that starts no reference")?;
                let name = &rest[..end];
                let whole = &raw[i..i + end + 2];
                let c = match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    _ => match name.strip_prefix('#') {
                        Some(n) => {
                            let code = match n.strip_prefix(['x', 'X']) {
                                Some(h) => u32::from_str_radix(h, 16),
                                None => n.parse(),
                            };
                            code.ok()
                                .and_then(char::from_u32)
                                .ok_or("a character reference that is no char")?
                        }
                        None => '\u{fffc}',
                    },
                };
                for _ in 0..end + 1 {
                    chars.next();
                }
                out.push(Tok {
                    c,
                    raw: Some(whole.to_string()),
                });
            }
            c => out.push(Tok { c, raw: None }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules {
        text: &["t"],
        paragraph: &["p"],
        breaks: &[("br", '\n'), ("softHyphen", '\u{ad}')],
        attrs: &[("*", "author"), ("docPr", "descr")],
        all_text: false,
        keep: &[],
    };

    const ALL: Rules = Rules {
        text: &["t"],
        paragraph: &["p"],
        breaks: &[("instrText", '\n')],
        attrs: &[],
        all_text: true,
        keep: &["instrText", "date"],
    };

    fn run(xml: &str) -> Result<String, String> {
        scrub(xml, &RULES, &Scrambler::new(5))
    }

    fn texts(xml: &str) -> Vec<String> {
        xml.split("<w:t")
            .skip(1)
            .map(|s| {
                let s = s.split_once('>').unwrap().1;
                s.split("</w:t>").next().unwrap().to_string()
            })
            .collect()
    }

    #[test]
    fn only_the_text_changes() {
        let xml = "<?xml version=\"1.0\"?>\n<w:p w:rsid='00A'  w:x=\"y\"><w:r><w:rPr><w:i/></w:rPr>\
                   <w:t xml:space=\"preserve\">Der neue Advokat &amp; die Freitreppe&#160;— 12</w:t></w:r></w:p>\n";
        let out = run(xml).unwrap();
        assert_ne!(out, xml);
        let strip = |s: &str| s.replace(&texts(s)[0], "");
        assert_eq!(strip(&out), strip(xml));
        let t = &texts(&out)[0];
        assert!(t.contains("&amp;") && t.contains("&#160;— 12"), "{t}");
        assert!(!t.contains("Advokat"));
    }

    #[test]
    fn a_word_split_across_runs_is_one_word() {
        let xml = "<w:p><w:r><w:t>gr</w:t></w:r><w:r><w:t>ü</w:t></w:r><w:r><w:t>n und gr</w:t></w:r>\
                   <w:r><w:t>ü</w:t></w:r><w:r><w:t>n</w:t></w:r></w:p>";
        let t = texts(&run(xml).unwrap());
        let all = t.concat();
        let words: Vec<&str> = all.split(' ').collect();
        assert_eq!(words[0], words[2], "{all}");
        assert_ne!(words[0], "grün");
        // "und" is a function word of the language count and stays.
        assert_eq!(words[1], "und");
    }

    #[test]
    fn a_break_ends_a_word_and_a_soft_hyphen_does_not() {
        // "Kapitel 3" is a label the importer reads: kept whole, not in halves.
        let join = |sep: &str| {
            let xml = format!("<w:p><w:r><w:t>Kapi</w:t>{sep}<w:t>tel 3</w:t></w:r></w:p>");
            texts(&run(&xml).unwrap()).concat()
        };
        assert_eq!(join(""), "Kapitel 3");
        // The importer drops a soft hyphen from a heading before it reads the
        // label (`heading::classify`), so "Kapi\u{ad}tel 3" is a label
        // to it as much as "Kapitel 3" is. The soft hyphen is a joiner: it
        // does not end the word, and the word is kept whole as a label, with
        // the `softHyphen` element where it was (the scrambled copy keeps
        // what the importer reads).
        assert_eq!(join("<w:softHyphen/>"), "Kapitel 3");
        // A word that is not a label is replaced in one piece across it.
        let prose = |sep: &str| {
            let xml = format!(
                "<w:p><w:r><w:t>Land</w:t>{sep}<w:t>arzt Land</w:t>{sep}<w:t>arzt</w:t></w:r></w:p>"
            );
            texts(&run(&xml).unwrap()).concat()
        };
        let words = prose("<w:softHyphen/>");
        let words: Vec<&str> = words.split(' ').collect();
        assert_eq!(words[0], words[1], "one word, one replacement");
        assert_ne!(words[0], "Landarzt");
        let split = prose("<w:br/>");
        let split: Vec<&str> = split.split(' ').collect();
        assert_ne!(split[0], "Landarzt");
        let broken = join("<w:br/>");
        assert!(
            !broken.contains("Kapi") && !broken.contains("tel"),
            "{broken}"
        );
    }

    #[test]
    fn attributes_are_scrambled_where_the_rules_say() {
        let xml = "<w:ins w:id=\"3\" w:author=\"Franz Kafka\" w:date=\"2026-01-02T03:04:05Z\"><wp:docPr id=\"1\" name=\"Bild 1\" descr=\"Ein Bild\"/></w:ins>";
        let out = run(xml).unwrap();
        assert!(out.contains("w:date=\"2026-01-02T03:04:05Z\"") && out.contains("name=\"Bild 1\""));
        assert!(!out.contains("Franz") && !out.contains("Bild\"") && out.contains("w:id=\"3\""));
    }

    #[test]
    fn text_outside_a_paragraph_is_its_own() {
        let out = run("<cp:coreProperties><w:t>Ein Landarzt</w:t></cp:coreProperties>").unwrap();
        assert!(!out.contains("Landarzt"));
    }

    #[test]
    fn all_text_scrambles_what_the_rules_do_not_name() {
        // A part whose elements the rules do not know: every text node,
        // comment and CDATA section is prose, except the kept elements.
        let xml = "<?xml version=\"1.0\"?>\n<b:Sources xmlns:b=\"x\">\n  <b:Source>\n\
                   <b:Title>Ein Landarzt</b:Title><b:Tag n=\"Bucephalus\"/><!-- Franz Kafka -->\
                   <b:Note><![CDATA[Doch sah ich & letzthin]]></b:Note>\
                   <b:date>2026-10-28T10:00:00Z</b:date><b:Extra>Landarzt &amp; Kutscher</b:Extra>\
                   </b:Source>\n</b:Sources>";
        let out = scrub(xml, &ALL, &Scrambler::new(5)).unwrap();
        for gone in ["Landarzt", "Franz", "Kafka", "letzthin", "Kutscher"] {
            assert!(!out.contains(gone), "{gone}\n{out}");
        }
        for kept in [
            "<b:Tag n=\"Bucephalus\"/>",
            "<b:date>2026-10-28T10:00:00Z</b:date>",
            "<![CDATA[",
            " & ",
            "&amp; ",
            "<!-- ",
            "\n  <b:Source>\n",
        ] {
            assert!(out.contains(kept), "{kept}\n{out}");
        }
        // Same length and same markup: only letters differ.
        assert_eq!(out.chars().count(), xml.chars().count());
    }

    #[test]
    fn all_text_joins_text_outside_the_named_elements_to_its_paragraph() {
        // "Landarzt" split across a named run and a bare text node is one
        // word.
        let out = scrub(
            "<p><t>Land</t>arzt und <t>Land</t>arzt</p>",
            &ALL,
            &Scrambler::new(5),
        )
        .unwrap();
        let text = out
            .replace("<p>", "")
            .replace("</p>", "")
            .replace("<t>", "")
            .replace("</t>", "");
        let words: Vec<&str> = text.split(' ').collect();
        assert_eq!(words[0], words[2], "{text}");
        assert_ne!(words[0], "Landarzt");
    }

    #[test]
    fn damaged_parts_are_errors_not_repairs() {
        for xml in [
            "<w:p><w:t>a</w:p></w:t>",
            "<w:p><w:t>a",
            "<w:p><w:t><![CDATA[a]]></w:t></w:p>",
            "<w:p><w:t>a &broken b</w:t></w:p>",
        ] {
            assert!(run(xml).is_err(), "{xml}");
        }
    }
}
