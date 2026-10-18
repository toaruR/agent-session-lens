//! Source root walking: discovers new files and drops vanished ones.

use super::sources::matches_source;
use super::{Engine, Parser, TrackedFile};
use crate::model::SourceState;
use crate::reader::Cursor;
use chrono::Utc;
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

impl Engine {
    /// Walks every source root, picking up new files and dropping vanished ones.
    pub fn rescan(&mut self) {
        let now = self.clock.now();
        let opener = self.opener.clone();
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        for (idx, src) in self.sources.iter().enumerate() {
            let info = &mut self.scans[idx];
            info.last_scan_at = Some(Utc::now());
            info.skipped_old = 0;
            info.detail = None;
            match std::fs::metadata(&src.root) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    info.state = Some(SourceState::NotInstalled);
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                    info.state = Some(SourceState::AccessDenied);
                    continue;
                }
                Err(e) => {
                    info.state = Some(SourceState::Error);
                    info.detail = Some(format!("{:?}", e.kind()));
                    continue;
                }
                Ok(_) => {}
            }
            if let Err(e) = opener(&src.root) {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    info.state = Some(SourceState::AccessDenied);
                } else {
                    info.state = Some(SourceState::Error);
                    info.detail = Some(format!("{:?}", e.kind()));
                }
                continue;
            }
            let mut matched = 0u64;
            let mut denied = false;
            for entry in walkdir::WalkDir::new(&src.root).max_depth(5) {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        if e.io_error().map(|io| io.kind()) == Some(io::ErrorKind::PermissionDenied) {
                            denied = true;
                        }
                        continue;
                    }
                };
                if !entry.file_type().is_file() || !matches_source(src.agent, entry.path()) {
                    continue;
                }
                matched += 1;
                let path = entry.path().to_path_buf();
                let meta = entry.metadata().ok();
                let mtime = meta.as_ref().and_then(|m| m.modified().ok());
                let recent = mtime.is_none_or(|m| now.duration_since(m).unwrap_or_default() <= self.max_age);
                match self.files.get(&path) {
                    Some(tf) => {
                        let size = meta.as_ref().map(|m| m.len());
                        if Some(tf.cursor.size) != size || tf.cursor.mtime != mtime {
                            self.dirty.insert(path.clone());
                        }
                        seen.insert(path);
                    }
                    None if recent => {
                        self.files.insert(
                            path.clone(),
                            TrackedFile { source: idx, cursor: Cursor::default(), parser: Parser::new(src.agent, &path), read_error: None },
                        );
                        self.dirty.insert(path.clone());
                        seen.insert(path);
                    }
                    None => info.skipped_old += 1,
                }
            }
            info.state = Some(if matched == 0 {
                if denied { SourceState::AccessDenied } else { SourceState::Empty }
            } else {
                SourceState::Ok
            });
            if denied && matched > 0 {
                info.detail = Some("some directories were not readable".into());
            }
        }
        self.files.retain(|p, _| seen.contains(p));
        self.dirty.retain(|p| seen.contains(p));
    }
}
