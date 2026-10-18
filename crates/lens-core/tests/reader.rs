//! Reader hardening tests (architecture-v5 §8). Fixtures are generated; no real log content.

use lens_core::reader::{self, Cursor, Limits};
use std::fs::{self, OpenOptions};
use std::io::Write;

const MAX: usize = 4194304;

fn lim() -> Limits {
    Limits::default()
}

#[test]
fn f4_invalid_utf8_line_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{\"a\":1}\n\x82\xa0\n{\"a\":2}\n").unwrap();
    let o = reader::read_new(&p, &mut Cursor::default(), &lim()).unwrap();
    assert_eq!(o.invalid_utf8_lines, 1);
    assert_eq!(o.lines.len(), 2);
    assert_eq!(o.lines[0].bytes, b"{\"a\":1}");
    assert_eq!(o.lines[1].bytes, b"{\"a\":2}");
    assert_eq!(o.lines[1].line_no, Some(2));
}

#[test]
fn f5_max_line_boundary_terminated() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    let mut data = vec![b'x'; MAX];
    data.push(b'\n');
    fs::write(&p, &data).unwrap();
    let o = reader::read_new(&p, &mut Cursor::default(), &lim()).unwrap();
    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.lines[0].bytes.len(), MAX);
    assert_eq!(o.oversized_lines, 0);

    let mut data = vec![b'x'; MAX + 1];
    data.extend_from_slice(b"\n{\"ok\":1}\n");
    fs::write(&p, &data).unwrap();
    let o = reader::read_new(&p, &mut Cursor::default(), &lim()).unwrap();
    assert_eq!(o.oversized_lines, 1);
    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.lines[0].bytes, b"{\"ok\":1}");
}

#[test]
fn f5_max_line_boundary_unterminated() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, vec![b'x'; MAX]).unwrap();
    let mut cur = Cursor::default();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert!(o.lines.is_empty());
    assert_eq!(o.oversized_lines, 0);
    assert_eq!(cur.offset(), 0);

    fs::write(&p, vec![b'x'; MAX + 1]).unwrap();
    let mut cur = Cursor::default();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert!(o.lines.is_empty());
    assert_eq!(o.oversized_lines, 1);
    // Rest of the oversized line is skipped and not counted twice.
    OpenOptions::new().append(true).open(&p).unwrap().write_all(b"yy\n{\"ok\":1}\n").unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.oversized_lines, 0);
    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.lines[0].bytes, b"{\"ok\":1}");
}

#[test]
fn f6_incomplete_tail_holds_offset() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{\"b\":").unwrap();
    let mut cur = Cursor::default();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert!(o.lines.is_empty());
    assert!(o.incomplete_tail);
    assert_eq!(cur.offset(), 0);
    OpenOptions::new().append(true).open(&p).unwrap().write_all(b"2}\n").unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.lines.len(), 1);
    assert!(!o.incomplete_tail);
    assert_eq!(cur.offset(), fs::metadata(&p).unwrap().len());
}

fn lines_3072() -> Vec<u8> {
    let mut d = Vec::new();
    for _ in 0..48 {
        d.extend_from_slice(&[b'a'; 63]);
        d.push(b'\n');
    }
    assert_eq!(d.len(), 3072);
    d
}

#[test]
fn f7_file_replaced_on_fileid_change_or_shrink() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    let q = tmp.path().join("b.jsonl");

    // fileId change: same size, different file swapped in.
    fs::write(&p, lines_3072()).unwrap();
    let mut cur = Cursor::default();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert!(!o.replaced);
    assert_eq!(o.lines.len(), 48);
    fs::write(&q, lines_3072()).unwrap();
    fs::rename(&q, &p).unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.replaced as u32, 1);
    assert_eq!(o.lines.len(), 48);
    assert_eq!(o.lines[0].line_no, Some(0));

    // size shrink 3072 -> 1000.
    fs::write(&p, lines_3072()).unwrap();
    let mut cur = Cursor::default();
    reader::read_new(&p, &mut cur, &lim()).unwrap();
    let mut small = Vec::new();
    while small.len() + 64 <= 1000 {
        small.extend_from_slice(&[b'b'; 63]);
        small.push(b'\n');
    }
    small.extend_from_slice(&vec![b'c'; 1000 - small.len()]);
    assert_eq!(small.len(), 1000);
    fs::write(&p, &small).unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.replaced as u32, 1);
    assert_eq!(o.lines.len(), 15);
    assert_eq!(o.lines[0].line_no, Some(0));
}

#[test]
fn f7_small_append_no_reread() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, lines_3072()).unwrap();
    let mut cur = Cursor::default();
    reader::read_new(&p, &mut cur, &lim()).unwrap();
    let mut add = vec![b'z'; 99];
    add.push(b'\n');
    OpenOptions::new().append(true).open(&p).unwrap().write_all(&add).unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.replaced as u32, 0);
    assert_eq!(o.lines.len(), 1);
    assert_eq!(o.lines[0].bytes.len(), 99);
    assert_eq!(o.lines[0].line_no, Some(48));
    assert_eq!(cur.offset(), 3172);
}

#[cfg(windows)]
#[test]
fn windows_share_mode_allows_concurrent_writer() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    fs::write(&p, b"{\"a\":1}\n").unwrap();
    let mut w = OpenOptions::new().append(true).share_mode(0x1 | 0x2 | 0x4).open(&p).unwrap();
    let mut cur = Cursor::default();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.lines.len(), 1);
    w.write_all(b"{\"a\":2}\n").unwrap();
    let o = reader::read_new(&p, &mut cur, &lim()).unwrap();
    assert_eq!(o.lines.len(), 1);
}
