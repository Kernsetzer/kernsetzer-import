//! `read_manuscript`: the file layer the CLI and the app share.

use std::fs;
use std::path::PathBuf;

use setzer_import::{ReadError, read_manuscript};

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn markdown_is_read_with_or_without_a_byte_order_mark() {
    let d = dir("file-bom");
    fs::write(d.join("a.md"), "\u{feff}# A Title\n\nText.\n").unwrap();
    fs::write(d.join("b.MD"), "# A Title\n\nText.\n").unwrap();
    let a = read_manuscript(&d.join("a.md")).unwrap();
    let b = read_manuscript(&d.join("b.MD")).unwrap();
    assert_eq!(a.book, b.book);
}

#[test]
fn other_files_and_bad_text_are_told_apart() {
    let d = dir("file-errors");
    fs::write(d.join("a.txt"), "x").unwrap();
    fs::write(d.join("b.md"), [0xff, 0xfe, 0x80]).unwrap();
    fs::write(d.join("c.docx"), "not a zip").unwrap();
    assert!(matches!(
        read_manuscript(&d.join("a.txt")),
        Err(ReadError::Unsupported)
    ));
    assert!(matches!(
        read_manuscript(&d.join("b.md")),
        Err(ReadError::NotUtf8)
    ));
    assert!(matches!(
        read_manuscript(&d.join("c.docx")),
        Err(ReadError::Import(_))
    ));
    assert!(matches!(
        read_manuscript(&d.join("missing.md")),
        Err(ReadError::Io(_))
    ));
}
