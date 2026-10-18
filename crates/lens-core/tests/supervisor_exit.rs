//! Supervisor exit-code and version policy (architecture-v5 sections 6.2, 6.3): F13a, F29a-F29e.
//! Time is a FakeClock; the helper is a fake replaying exact stdout sequences and exit codes.

use lens_core::clock::FakeClock;
use lens_core::model::v5::SourceState;
use lens_core::settings::Settings;
use lens_core::supervisor::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const HELLO1: &str = r#"{"type":"hello","protocol":1,"helperVersion":"0.1","distro":"Ubuntu","home":"/home/u","now":"2026-01-01T00:00:00Z"}"#;
const HELLO2: &str = r#"{"type":"hello","protocol":2,"helperVersion":"0.1","distro":"Ubuntu","home":"/home/u","now":"2026-01-01T00:00:00Z"}"#;

#[test]
fn output_delivered_during_exit_poll_is_ingested_before_drop() {
    struct TailLauncher;
    struct TailProcess { polled: bool }
    impl HelperProcess for TailProcess {
        fn poll_exit(&mut self) -> Option<HelperExit> {
            self.polled = true;
            Some(HelperExit::Code(EXIT_USAGE))
        }
        fn read_stdout(&mut self) -> Vec<u8> {
            if self.polled {
                format!("{}\n{}\n", HELLO1, upsert("tail")).into_bytes()
            } else {
                Vec::new()
            }
        }
        fn kill(&mut self) {}
    }
    impl Launcher for TailLauncher {
        fn launch(&self, _: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError> {
            Ok(Box::new(TailProcess { polled: false }))
        }
    }
    let mut s = Supervisor::new("Ubuntu", &Settings::default(), Arc::new(TailLauncher),
        Arc::new(FakeClock::new(SystemTime::UNIX_EPOCH)));
    s.tick(true);
    s.tick(true);
    assert_eq!(s.sessions().len(), 1);
    assert_eq!(s.health().unwrap().top_diagnostics[0].code, USAGE_ERROR);
}

fn upsert(id: &str) -> String {
    format!(
        r#"{{"type":"session_upsert","session":{{"sessionUid":"wsl:Ubuntu:claude:{id}","nativeId":"{id}","agent":"claude_code","sourceId":"wsl:Ubuntu:claude","origin":{{"kind":"wsl","distro":"Ubuntu"}},"lastActivitySource":"event","projects":[],"status":{{"value":"idle","reason":"r","confidence":"high","stale":false,"autoReview":false}},"currentTask":{{"title":"t"}},"hierarchy":{{"childUids":[],"kind":"top_level"}},"health":{{"completeness":"complete","diagnostics":[]}}}}}}"#
    )
}

/// One scripted helper run: stdout lines, then (optionally) an exit code.
#[derive(Clone)]
struct Run {
    lines: Vec<String>,
    exit: Option<i32>,
}

#[derive(Clone, Default)]
struct FakeLauncher {
    runs: Arc<Mutex<VecDeque<Run>>>,
    launches: Arc<Mutex<usize>>,
}

struct FakeProc {
    run: Run,
    sent: bool,
}

impl HelperProcess for FakeProc {
    fn poll_exit(&mut self) -> Option<HelperExit> {
        if self.sent {
            self.run.exit.map(HelperExit::Code)
        } else {
            None
        }
    }
    fn kill(&mut self) {}
    fn read_stdout(&mut self) -> Vec<u8> {
        self.sent = true;
        let mut b = Vec::new();
        for l in std::mem::take(&mut self.run.lines) {
            b.extend_from_slice(l.as_bytes());
            b.push(b'\n');
        }
        b
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, _argv: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError> {
        *self.launches.lock().unwrap() += 1;
        let run = self.runs.lock().unwrap().pop_front().expect("unscripted launch");
        Ok(Box::new(FakeProc { run, sent: false }))
    }
}

fn setup(runs: Vec<Run>) -> (Supervisor, FakeLauncher, Arc<FakeClock>) {
    let l = FakeLauncher { runs: Arc::new(Mutex::new(runs.into())), ..Default::default() };
    let c = Arc::new(FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)));
    (Supervisor::new("Ubuntu", &Settings::default(), Arc::new(l.clone()), c.clone()), l, c)
}

fn n(l: &FakeLauncher) -> usize {
    *l.launches.lock().unwrap()
}

fn exit_run(code: i32) -> Run {
    Run { lines: vec![HELLO1.into()], exit: Some(code) }
}

fn no_restart_after(s: &mut Supervisor, l: &FakeLauncher, c: &FakeClock) {
    for _ in 0..5 {
        c.advance(Duration::from_secs(120));
        s.tick(true);
    }
    assert_eq!(n(l), 1);
    assert_eq!(s.restarts(), 0);
}

#[test]
fn f13a_hello_protocol_2_no_restart() {
    let (mut s, l, c) = setup(vec![Run { lines: vec![HELLO2.into()], exit: None }]);
    s.tick(true);
    s.tick(true);
    assert_eq!(s.health().unwrap().state, SourceState::HelperVersionMismatch);
    assert_eq!(s.restarts(), 0);
    no_restart_after(&mut s, &l, &c);
}

#[test]
fn f29a_exit_2_usage_error_no_restart() {
    let (mut s, l, c) = setup(vec![Run { lines: vec![], exit: Some(2) }]);
    s.tick(true);
    s.tick(true);
    s.tick(true);
    let h = s.health().unwrap();
    assert_eq!(h.state, SourceState::HelperUnavailable);
    assert_eq!(h.top_diagnostics[0].code, "usage_error");
    no_restart_after(&mut s, &l, &c);
}

#[test]
fn f29b_exit_3_version_mismatch_no_restart() {
    let (mut s, l, c) = setup(vec![Run { lines: vec![], exit: Some(3) }]);
    s.tick(true);
    s.tick(true);
    s.tick(true);
    assert_eq!(s.health().unwrap().state, SourceState::HelperVersionMismatch);
    no_restart_after(&mut s, &l, &c);
}

fn restart_after_1s(code: i32) {
    let (mut s, l, c) = setup(vec![exit_run(code), exit_run(code)]);
    s.tick(true);
    s.tick(true); // reads output, then sees the exit
    assert_eq!((s.restarts(), n(&l)), (1, 1));
    s.tick(true);
    assert_eq!(n(&l), 1, "0 s: no launch");
    c.advance(Duration::from_millis(999));
    s.tick(true);
    assert_eq!(n(&l), 1);
    c.advance(Duration::from_millis(1));
    s.tick(true);
    assert_eq!(n(&l), 2, "1 s: one launch");
}

#[test]
fn f29c_exit_4_restarts_after_1s() {
    restart_after_1s(4);
}

#[test]
fn f29d_exit_5_restarts_after_1s() {
    restart_after_1s(5);
}

#[test]
fn f29e_exit_101_restart_resends_hello_and_upserts() {
    let full = |exit| Run { lines: vec![HELLO1.into(), upsert("a"), upsert("b")], exit };
    let (mut s, l, c) = setup(vec![full(Some(101)), full(None)]);
    s.tick(true);
    s.tick(true);
    assert_eq!(s.receiver().sessions().len(), 2);
    assert_eq!(s.restarts(), 1);
    c.advance(Duration::from_secs(1));
    s.tick(true);
    assert_eq!(n(&l), 2);
    s.tick(true);
    assert!(s.receiver().hello().is_some());
    assert_eq!(s.receiver().sessions().len(), 2);
    assert_eq!(s.restarts(), 1);
}
