//! Real OS pipes, including backpressure and output written just before exit.
use lens_core::supervisor::{HelperExit, HelperProcess, Launcher, WslLauncher};
use std::io::Write;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Box<dyn HelperProcess> {
    WslLauncher.launch(&[
        std::env::current_exe().unwrap().to_string_lossy().into_owned(),
        "--exact".into(), name.into(), "--ignored".into(), "--nocapture".into(),
    ]).unwrap_or_else(|_| panic!("launch fixture"))
}

#[test]
#[ignore = "subprocess fixture"]
fn streaming_fixture() {
    let mut out = std::io::stdout();
    out.write_all(b"STREAM_READY\n").unwrap();
    out.flush().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    std::io::stderr().write_all(&vec![b'e'; 200_000]).unwrap();
    out.write_all(&vec![b'x'; 600_000]).unwrap();
    out.write_all(b"STREAM_END\n").unwrap();
}

#[test]
#[ignore = "subprocess fixture"]
fn missing_fixture() {
    eprintln!("execv: No such file or directory");
    std::process::exit(127);
}

#[test]
fn real_pipes_stream_before_exit_and_keep_final_output() {
    let mut child = fixture("streaming_fixture");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut output = Vec::new();
    let mut saw_live_output = false;
    loop {
        let start = Instant::now();
        output.extend(child.read_stdout());
        assert!(start.elapsed() < Duration::from_secs(1), "read must not block");
        if let Some(exit) = child.poll_exit() {
            output.extend(child.read_stdout());
            assert!(matches!(exit, HelperExit::Code(0)));
            break;
        }
        saw_live_output |= output.windows(12).any(|w| w == b"STREAM_READY");
        assert!(Instant::now() < deadline, "pipe reader stalled");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_live_output, "stdout must arrive while the helper is alive");
    assert_eq!(output.split(|&b| b != b'x').map(|run| run.len()).max(), Some(600_000));
    assert!(output.windows(10).any(|w| w == b"STREAM_END"));
}

#[test]
fn real_exit_classifies_missing_helper() {
    let mut child = fixture("missing_fixture");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        child.read_stdout();
        if let Some(exit) = child.poll_exit() {
            assert!(matches!(exit, HelperExit::NotFound));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires running WSL distro with installed helper and existing session logs"]
fn real_wsl_ingests_existing_sessions() {
    verify_wsl(true);
}

#[test]
#[ignore = "requires running WSL distro with installed helper"]
fn real_wsl_reads_existing_sources() {
    verify_wsl(false);
}

fn verify_wsl(require_sessions: bool) {
    use lens_core::{clock::SystemClock, settings::Settings, supervisor::Supervisor};
    use std::sync::Arc;
    let distro = std::env::var("LENS_TEST_DISTRO").unwrap_or_else(|_| "Ubuntu".into());
    let mut settings = Settings::default();
    for (agent, key) in [("claude", "LENS_TEST_CLAUDE_ROOT"), ("codex", "LENS_TEST_CODEX_ROOT")] {
        if let Ok(root) = std::env::var(key) {
            settings.source_roots.insert(format!("wsl:{distro}:{agent}"), root);
        }
    }
    let mut supervisor = Supervisor::new(&distro, &settings, Arc::new(WslLauncher), Arc::new(SystemClock));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        supervisor.tick(true);
        let sessions = supervisor.sessions();
        let health = supervisor.receiver().source_health();
        if (!require_sessions || !sessions.is_empty()) && health.len() >= 2 {
            println!("WSL {}: {} sessions, {} source health reports", distro, sessions.len(), health.len());
            for source in health.values() {
                println!("{}: {:?}, {} files, {} sessions, {} skipped old files", source.source_id, source.state, source.files_tracked, source.sessions_total, source.skipped_old_files);
            }
            assert!(supervisor.health().is_none(), "helper must be available");
            supervisor.tick(false);
            return;
        }
        assert!(Instant::now() < deadline,
            "WSL did not deliver sessions: helper={:?}, sessions={}, sources={:?}, diagnostics={:?}",
            supervisor.health(), sessions.len(), health, supervisor.receiver().diagnostics());
        std::thread::sleep(Duration::from_millis(100));
    }
}
