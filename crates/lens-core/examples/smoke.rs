//! Real-log smoke run. Prints aggregate counts only — never message bodies, titles or paths.
//!
//!     cargo run -p lens-core --example smoke -- --real-logs

use lens_core::model::{SessionKind, StatusValue};
use lens_core::{default_sources, Engine};
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    if !std::env::args().any(|a| a == "--real-logs") {
        eprintln!("refusing to read real logs without --real-logs");
        std::process::exit(2);
    }
    let started = Instant::now();
    let mut engine = Engine::new(default_sources());
    engine.rescan();
    engine.process_all();
    let snap = engine.snapshot(chrono::Utc::now());
    println!("elapsed_ms={}", started.elapsed().as_millis());

    for src in &snap.sources {
        println!(
            "source={} state={:?} files={} skipped_old={} sessions={} partial={} versions={} top_diag={:?}",
            src.source_id,
            src.state,
            src.files_tracked,
            src.files_skipped_old,
            src.sessions_total,
            src.partial_sessions,
            src.writer_versions.len(),
            src.top_diagnostics.iter().map(|d| (&d.code, d.count)).collect::<Vec<_>>()
        );
    }
    let mut by: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut no_project = 0;
    for s in &snap.sessions {
        let kind = match s.hierarchy.kind {
            SessionKind::TopLevel => "top",
            SessionKind::Subagent => "sub",
            SessionKind::Fork => "fork",
            SessionKind::Automated => "auto",
        };
        let status = match s.status.value {
            StatusValue::Running if s.status.stale => "running(stale)",
            StatusValue::Running => "running",
            StatusValue::WaitingInput => "waiting_input",
            StatusValue::Idle => "idle",
            StatusValue::Error => "error",
            StatusValue::Unknown => "unknown",
        };
        *by.entry((s.source_id.clone(), kind.into(), status.into())).or_default() += 1;
        if s.projects.is_empty() {
            no_project += 1;
        }
    }
    for ((src, kind, status), n) in by {
        println!("{src} {kind} {status} {n}");
    }
    let with_title = snap.sessions.iter().filter(|s| s.current_task.title.is_some()).count();
    println!("sessions={} with_title={} no_project={}", snap.sessions.len(), with_title, no_project);
}
