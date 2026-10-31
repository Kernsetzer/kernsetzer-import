//! Small helpers over quick-xml events. Names are matched by local name:
//! .docx producers agree on namespaces but not always on prefixes.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesRef, BytesStart};

/// Attribute value by local name (`w:val` → `"val"`), unescaped.
pub(super) fn attr(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .find(|a| a.key.local_name().into_inner() == local)
        .map(|a| match a.normalized_value(XmlVersion::Implicit1_0) {
            Ok(v) => v.into_owned(),
            Err(_) => a.value.into_owned(),
        })
}

/// `w:val` of an element.
pub(super) fn val(e: &BytesStart) -> Option<String> {
    attr(e, "val")
}

/// An on/off property: `<w:i/>` and `<w:i w:val="true"/>` are on,
/// `w:val="0"`, `"false"`, `"off"` and `"none"` are off.
pub(super) fn on_off(e: &BytesStart) -> bool {
    !matches!(val(e).as_deref(), Some("0" | "false" | "off" | "none"))
}

/// Integer attribute (twips, EMU, ids). Accepts decimals some producers
/// write ("720.0").
pub(super) fn int_attr(e: &BytesStart, local: &str) -> Option<i64> {
    let v = attr(e, local)?;
    v.trim()
        .parse::<i64>()
        .ok()
        .or_else(|| v.trim().parse::<f64>().ok().map(|f| f as i64))
}

/// Text of an entity or character reference (`&amp;`, `&#x2014;`).
pub(super) fn resolve_ref(r: &BytesRef) -> Option<char> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return Some(c);
    }
    match &**r {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::Reader;
    use quick_xml::events::Event;

    fn first(xml: &str) -> BytesStart<'_> {
        let mut r = Reader::from_str(xml);
        loop {
            match r.read_event().unwrap() {
                Event::Start(e) | Event::Empty(e) => return e,
                Event::Eof => panic!("no element"),
                _ => {}
            }
        }
    }

    #[test]
    fn on_off_values() {
        assert!(on_off(&first(r#"<w:i/>"#)));
        assert!(on_off(&first(r#"<w:i w:val="1"/>"#)));
        assert!(!on_off(&first(r#"<w:i w:val="0"/>"#)));
        assert!(!on_off(&first(r#"<w:b w:val="false"/>"#)));
    }

    #[test]
    fn attributes_by_local_name() {
        let e = first(r#"<w:ind w:firstLine="720.0" w:left="10" r:id="a&amp;b"/>"#);
        assert_eq!(int_attr(&e, "firstLine"), Some(720));
        assert_eq!(attr(&e, "id").as_deref(), Some("a&b"));
        assert_eq!(attr(&e, "missing"), None);
    }
}
