//! Supervisor lifecycle (architecture-v5 sections 3.3, 6, 6.1): F11a, F11b, F12, launch command,
//! helper_not_found. Time is a FakeClock and the launcher is a fake; no real WSL.

use chrono::{DateTime, Duration as CDuration, Utc};
use lens_core::aggregator::{origin_for_source, Aggregator, SessionInput};
use lens_core::clock::FakeClock;
use lens_core::model::v5::{SessionProject, SourceState};
use lens_core::model::{Agent, Attribution, SessionKind};
use lens_core::session::SessionState;
use lens_core::settings::Settings;
use lens_core::supervisor::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Default)]
struct Shared {
    argvs: Vec<Vec<String>>,
    /// Scripted launch results; empty = succeed.
    script: VecDeque<Result<(), LaunchError>>,
    exit_now: bool,
    kills: u32,
}

#[derive(Clone, Default)]
struct FakeLauncher(Arc<Mutex<Shared>>);

struct FakeProc(Arc<Mutex<Shared>>);

impl HelperProcess for FakeProc {
    fn poll_exit(&mut self) -> Option<HelperExit> {
        let mut g = self.0.lock().unwrap();
        if g.exit_now {
            g.exit_now = false;
            Some(HelperExit::Code(1))
        } else {
            None
        }
    }
    fn kill(&mut self) {
        self.0.lock().unwrap().kills += 1;
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, argv: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError> {
        let mut g = self.0.lock().unwrap();
        g.argvs.push(argv.to_vec());
        match g.script.pop_front().unwrap_or(Ok(())) {
            Ok(()) => Ok(Box::new(FakeProc(self.0.clone()))),
            Err(e) => Err(e),
        }
    }
}

impl FakeLauncher {
    fn launches(&self) -> usize {
        self.0.lock().unwrap().argvs.len()
    }
}

fn start() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
}

fn setup(settings: &Settings) -> (Supervisor, FakeLauncher, Arc<FakeClock>) {
    let l = FakeLauncher::default();
    let c = Arc::new(FakeClock::new(start()));
    (Supervisor::new("Ubuntu", settings, Arc::new(l.clone()), c.clone()), l, c)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn f11a_heartbeat_silence_29_vs_30() {
    let (mut s, l, c) = setup(&Settings::default());
    s.tick(true);
    assert_eq!((s.launches(), s.restarts()), (1, 0));
    c.advance(secs(29));
    s.tick(true);
    assert_eq!(s.restarts(), 0);
    // Re-evaluating a running distro never launches a duplicate.
    s.tick(true);
    assert_eq!(l.launches(), 1);

    let (mut s, l, c) = setup(&Settings::default());
    s.tick(true);
    c.advance(secs(30));
    s.tick(true);
    assert_eq!(s.restarts(), 1);
    assert_eq!(l.0.lock().unwrap().kills, 1);
    // Relaunch only after backoff_secs(1) = 1 s.
    assert_eq!(l.launches(), 1);
    c.advance(secs(1));
    s.tick(true);
    assert_eq!(l.launches(), 2);
    assert_eq!(s.generation(), 2);
}

#[test]
fn heartbeat_keeps_helper_alive_and_exit_restarts() {
    let (mut s, l, c) = setup(&Settings::default());
    s.tick(true);
    for _ in 0..4 {
        c.advance(secs(20));
        s.on_alive();
        s.tick(true);
    }
    assert_eq!((s.restarts(), l.launches()), (0, 1));
    l.0.lock().unwrap().exit_now = true;
    s.tick(true);
    assert_eq!(s.restarts(), 1);
}

#[test]
fn f11b_eight_failures_backoff_waits() {
    let (mut s, l, c) = setup(&Settings::default());
    l.0.lock().unwrap().script = (0..8).map(|_| Err(LaunchError::Other)).collect();
    s.tick(true);
    for _ in 0..7 {
        // Waiting out less than the scheduled wait must not launch.
        s.tick(true);
        let before = l.launches();
        let w = *s.waits().last().unwrap();
        c.advance(secs(w - 1));
        s.tick(true);
        assert_eq!(l.launches(), before);
        c.advance(secs(1));
        s.tick(true);
    }
    assert_eq!(s.waits(), &[1, 2, 4, 8, 16, 32, 60, 60]);
    assert_eq!(s.restarts(), 0);

    // Launch now succeeds; a failure after one success waits 1 s again.
    c.advance(secs(60));
    s.tick(true);
    assert_eq!(s.launches(), 1);
    s.on_alive();
    l.0.lock().unwrap().exit_now = true;
    s.tick(true);
    assert_eq!(*s.waits().last().unwrap(), 1);
}

fn wsl_input() -> SessionInput {
    let t = |s: i64| -> DateTime<Utc> { "2026-10-17T00:00:00Z".parse::<DateTime<Utc>>().unwrap() + CDuration::seconds(s) };
    let mut st = SessionState::new("s1", SessionKind::TopLevel);
    st.touch(Some(t(0)));
    st.human_message("go", Some(t(0)));
    st.tool_start("c1".into(), "Bash", "ls", Some(t(5)));
    st.touch(Some(t(5)));
    SessionInput {
        source_id: "wsl:Ubuntu:claude".into(),
        agent: Agent::ClaudeCode,
        origin: origin_for_source("wsl:Ubuntu:claude"),
        state: st,
        file_mtime: None,
        projects: vec![SessionProject {
            key: "/home/u/repo".into(),
            name: "repo".into(),
            display_path: "/home/u/repo".into(),
            attribution: Attribution::GitRoot,
        }],
    }
}

#[test]
fn f12_stopped_distro_state() {
    let (mut s, l, c) = setup(&Settings::default());
    let now: DateTime<Utc> = "2026-10-17T00:00:10Z".parse().unwrap();
    let clock = Arc::new(FakeClock::new(SystemTime::from(now)));
    let mut agg = Aggregator::new(clock);
    agg.upsert(wsl_input());
    let before = agg.list_projects().remove(0);
    assert!(before.updating && before.origin_available);

    s.tick(true);
    assert_eq!(l.launches(), 1);
    // Distro stops: the running helper is killed and nothing is launched again.
    s.tick(false);
    c.advance(secs(600));
    s.tick(false);
    assert_eq!(l.launches(), 1, "no wsl.exe -d launch while stopped");
    assert!(s.is_stopped());
    agg.set_distro_available("Ubuntu", s.origin_available());

    let after = agg.list_projects().remove(0);
    assert!(!after.origin_available);
    assert!(!after.updating);
    assert_eq!(after.updating_confidence, None);
    assert_eq!(after.last_updated_at, before.last_updated_at);
    let h = s.health().unwrap();
    assert_eq!(h.state, SourceState::DistroStopped);

    // Back to running: launches again, health is no longer supervisor-owned.
    s.tick(true);
    assert_eq!(l.launches(), 2);
    assert!(s.health().is_none());
}

#[test]
fn stopped_from_start_never_launches() {
    let (mut s, l, _c) = setup(&Settings::default());
    s.tick(false);
    s.tick(false);
    assert_eq!(l.launches(), 0);
}

#[test]
fn launch_command_line() {
    let argv = launch_argv("Ubuntu", &Settings::default());
    assert_eq!(
        argv,
        ["wsl.exe", "-d", "Ubuntu", "--exec", "/usr/local/bin/lens-wsl-helper", "serve", "--protocol", "1"]
    );
    let mut st = Settings::default();
    st.source_roots.insert("wsl:Ubuntu:claude".into(), "/home/u/x".into());
    st.helper_path = Some("/opt/h".into());
    let argv = launch_argv("Ubuntu", &st);
    assert_eq!(
        argv,
        ["wsl.exe", "-d", "Ubuntu", "--exec", "/opt/h", "serve", "--protocol", "1", "--roots", "/home/u/x"]
    );
    // The supervisor launches exactly this argv.
    let (mut s, l, _c) = setup(&st);
    s.tick(true);
    assert_eq!(l.0.lock().unwrap().argvs[0], argv);
}

#[test]
fn missing_helper_is_helper_not_found() {
    let (mut s, l, c) = setup(&Settings::default());
    l.0.lock().unwrap().script = (0..3).map(|_| Err(LaunchError::NotFound)).collect();
    s.tick(true);
    let h = s.health().unwrap();
    assert_eq!(h.state, SourceState::HelperUnavailable);
    assert_eq!(h.top_diagnostics[0].code, "helper_not_found");
    // No restart loop beyond the backoff: ticking without advancing time does not relaunch.
    for _ in 0..5 {
        s.tick(true);
    }
    assert_eq!(l.launches(), 1);
    c.advance(secs(1));
    s.tick(true);
    assert_eq!(l.launches(), 2);
    assert_eq!(s.waits(), &[1, 2]);
}
