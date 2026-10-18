//! Read-only JSONL tail reader (architecture-v3 §5.1, v5 §8).
//!
//! The file handle is opened with full sharing and closed before returning, so
//! the writer can append, rotate or delete the file at any time.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_line: usize,
    pub max_read_per_pass: u64,
    pub initial_head: u64,
    pub initial_tail: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_line: crate::consts::MAX_LINE_BYTES,
            max_read_per_pass: crate::consts::TAIL_SCAN_BYTES,
            initial_head: crate::consts::HEAD_SCAN_BYTES,
            initial_tail: crate::consts::TAIL_SCAN_BYTES,
        }
    }
}

/// One complete line. `line_no` is 0-based and `None` once the reader has
/// skipped the middle of the file and no longer knows the absolute position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub line_no: Option<u64>,
    pub bytes: Vec<u8>,
    /// Invalid-UTF-8 lines dropped immediately before this one (keeps record order for the engine).
    pub invalid_before: u32,
}

#[derive(Debug, Default)]
pub struct ReadOutcome {
    pub lines: Vec<Line>,
    /// The file was replaced (different file id or shrank); caller must reset parsed state.
    pub replaced: bool,
    /// The initial scan skipped the middle of the file.
    pub truncated_initial_scan: bool,
    /// The initial tail window held no complete line (§3.1 rule 1).
    pub tail_empty: bool,
    /// More bytes remain; read again on the next pass.
    pub more: bool,
    pub oversized_lines: u64,
    /// Complete lines dropped because they were not strict UTF-8.
    pub invalid_utf8_lines: u64,
    /// The file ends in a line without a newline; it is held back (offset not advanced).
    pub incomplete_tail: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Cursor {
    file_id: Option<file_id::FileId>,
    offset: u64,
    next_line_no: Option<u64>,
    discarding: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    initialized: bool,
}

impl Cursor {
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

fn open_shared(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        opts.share_mode(0x1 | 0x2 | 0x4);
    }
    opts.open(path)
}

/// Reads complete lines appended since the last call.
pub fn read_new(path: &Path, cur: &mut Cursor, limits: &Limits) -> io::Result<ReadOutcome> {
    let mut out = ReadOutcome::default();
    let id = file_id::get_file_id(path).ok();
    let mut file = open_shared(path)?;
    let meta = file.metadata()?;
    let size = meta.len();

    if cur.initialized {
        let id_changed = matches!((&cur.file_id, &id), (Some(a), Some(b)) if a != b);
        if id_changed || size < cur.offset {
            *cur = Cursor::default();
            out.replaced = true;
        }
    }
    cur.file_id = id;
    cur.size = size;
    cur.mtime = meta.modified().ok();

    if !cur.initialized {
        cur.initialized = true;
        cur.next_line_no = Some(0);
        if size > limits.initial_head + limits.initial_tail {
            // Head: enough for session metadata and the first human message.
            let head = read_range(&mut file, 0, limits.initial_head)?;
            let mut head_cur = Cursor { next_line_no: Some(0), ..Cursor::default() };
            split_lines(&head, &mut head_cur, limits, &mut out);
            out.incomplete_tail = false;
            // Tail: the most recent activity. Drop the first partial line.
            let start = size - limits.initial_tail;
            let tail = read_range(&mut file, start, limits.initial_tail)?;
            let skip = tail.iter().position(|&b| b == b'\n').map(|p| p + 1).unwrap_or(tail.len());
            cur.next_line_no = None;
            cur.offset = start + skip as u64;
            let before = out.lines.len();
            let consumed = split_lines(&tail[skip..], cur, limits, &mut out);
            out.tail_empty = out.lines.len() == before;
            cur.offset += consumed;
            out.truncated_initial_scan = true;
            return Ok(out);
        }
    }

    if size <= cur.offset {
        return Ok(out);
    }
    let want = (size - cur.offset).min(limits.max_read_per_pass);
    let buf = read_range(&mut file, cur.offset, want)?;
    out.more = cur.offset + want < size;
    let consumed = split_lines(&buf, cur, limits, &mut out);
    cur.offset += consumed;
    if out.incomplete_tail && out.more {
        // The tail may just be the read-window edge, not the end of the file.
        out.incomplete_tail = false;
    }
    if consumed == 0 && out.more {
        // Window holds no complete line; avoid spinning on the same bytes.
        out.more = false;
    }
    Ok(out)
}

fn read_range(file: &mut File, start: u64, len: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity(len as usize);
    file.take(len).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Splits `buf` into complete lines and returns the number of bytes consumed.
///
/// Only bytes up to and including the last newline are consumed, so the
/// offset never advances into an incomplete tail (it is re-read next pass).
/// A newline-less tail longer than `max_line` is consumed and discarded up to
/// the next newline (counted once as oversized). Lines that are not strict
/// UTF-8 are dropped and counted; no lossy decoding is performed.
pub(crate) fn split_lines(buf: &[u8], cur: &mut Cursor, limits: &Limits, out: &mut ReadOutcome) -> u64 {
    let mut rest = buf;
    let mut invalid_before = 0u32;
    if cur.discarding {
        match rest.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                rest = &rest[pos + 1..];
                cur.discarding = false;
                cur.next_line_no = cur.next_line_no.map(|n| n + 1);
            }
            None => return buf.len() as u64,
        }
    }
    while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
        let mut line = &rest[..pos];
        rest = &rest[pos + 1..];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if line.len() > limits.max_line {
            out.oversized_lines += 1;
        } else if std::str::from_utf8(line).is_err() {
            out.invalid_utf8_lines += 1;
            invalid_before += 1;
        } else if !line.iter().all(|b| b.is_ascii_whitespace()) {
            out.lines.push(Line { line_no: cur.next_line_no, bytes: line.to_vec(), invalid_before: std::mem::take(&mut invalid_before) });
        }
        cur.next_line_no = cur.next_line_no.map(|n| n + 1);
    }
    if rest.len() > limits.max_line {
        out.oversized_lines += 1;
        cur.discarding = true;
        return buf.len() as u64;
    }
    if !rest.is_empty() {
        out.incomplete_tail = true;
    }
    (buf.len() - rest.len()) as u64
}
