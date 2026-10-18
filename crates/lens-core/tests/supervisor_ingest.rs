//! Supervisor ingest path (architecture-v5 sections 6.1, 6.3, 10): F31, F32, clock offset.
//! Fake clock + fake launcher; no real WSL.
use chrono::{DateTime, Duration as CDuration, TimeZone, Utc};
use lens_core::clock::FakeClock;
use lens_core::distro::{Distro, DistroMonitor, LxssRegistry, RunningProbe};
use lens_core::model::v5::wsl_source_id;
use lens_core::model::Agent;
use lens_core::settings::Settings;
use lens_core::supervisor::*;
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Default)]
struct Shared {
    argvs: Vec<Vec<String>>,
    /// stdout bytes handed to each successive launch.
    outputs: VecDeque<Vec<u8>>,
    exit_now: bool,
}

#[derive(Clone, Default)]
struct FakeLauncher(Arc<Mutex<Shared>>);

struct FakeProc {
    sh: Arc<Mutex<Shared>>,
    out: Vec<u8>,
}

impl HelperProcess for FakeProc {
    fn poll_exit(&mut self) -> Option<HelperExit> {
        let mut g = self.sh.lock().unwrap();
        if g.exit_now {
            g.exit_now = false;
            Some(HelperExit::Code(1))
        } else {
            None
        }
    }
    fn kill(&mut self) {}
    fn read_stdout(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, argv: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError> {
        let mut g = self.0.lock().unwrap();
        g.argvs.push(argv.to_vec());
        let out = g.outputs.pop_front().unwrap_or_default();
        Ok(Box::new(FakeProc { sh: self.0.clone(), out }))
    }
}

impl FakeLauncher {
    fn argvs(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().argvs.clone()
    }
    fn push_output(&self, s: String) {
        self.0.lock().unwrap().outputs.push_back(s.into_bytes());
    }
}

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
}

fn setup(settings: &Settings) -> (Supervisor, FakeLauncher, Arc<FakeClock>) {
    let l = FakeLauncher::default();
    let c = Arc::new(FakeClock::new(SystemTime::from(t0())));
    (Supervisor::new("Ubuntu", settings, Arc::new(l.clone()), c.clone()), l, c)
}

fn hello(now: DateTime<Utc>) -> String {
    format!(
        r#"{{"type":"hello","protocol":1,"helperVersion":"0.1","distro":"Ubuntu","home":"/home/u","now":"{}"}}"#,
        now.to_rfc3339()
    )
}

fn upsert(title: &str, last: Option<DateTime<Utc>>) -> String {
    let la = last.map(|d| format!(r#""lastActivityAt":"{}","#, d.to_rfc3339())).unwrap_or_default();
    format!(
        r#"{{"type":"session_upsert","session":{{"sessionUid":"wsl:Ubuntu:claude:s1","nativeId":"s1","agent":"claude_code","sourceId":"wsl:Ubuntu:claude","origin":{{"kind":"wsl","distro":"Ubuntu"}},{la}"lastActivitySource":"event","projects":[],"status":{{"value":"idle","reason":"r","confidence":"high","stale":false,"autoReview":false}},"currentTask":{{"title":"{title}"}},"hierarchy":{{"childUids":[],"kind":"top_level"}},"health":{{"completeness":"complete","diagnostics":[]}}}}}}"#
    )
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn f31_leaked_token_redacted_before_ipc() {
    let (mut s, l, _c) = setup(&Settings::default());
    let tok = format!("ghp_{}", "a1B2".repeat(9));
    l.push_output(format!("{}\n{}\n", hello(t0()), upsert(&format!("leak {tok}"), None)));
    s.tick(true);
    s.tick(true);
    let ss = s.sessions();
    assert_eq!(ss.len(), 1);
    let title = ss[0].current_task.title.clone().unwrap();
    assert!(title.contains("[REDACTED]"), "{title}");
    assert!(!title.contains(&tok), "{title}");
}

#[test]
fn f32_helper_path_applies_at_next_launch() {
    let (mut s, l, c) = setup(&Settings::default());
    s.tick(true);
    assert!(!l.argvs()[0].contains(&"/opt/h".to_string()));
    let st = Settings { helper_path: Some("/opt/h".into()), ..Settings::default() };
    s.update_settings(&st);
    s.tick(true);
    assert_eq!(l.argvs().len(), 1);
    // helper dies; restart after backoff picks up the new path.
    l.0.lock().unwrap().exit_now = true;
    s.tick(true);
    for _ in 0..70 {
        c.advance(secs(1));
        s.tick(true);
    }
    let a = l.argvs();
    assert!(a.len() >= 2);
    assert!(!a[0].contains(&"/opt/h".to_string()));
    assert!(a[1].contains(&"/opt/h".to_string()), "{:?}", a[1]);
}

#[test]
fn f32_source_roots_apply_at_next_launch() {
    let id = wsl_source_id("Ubuntu", Agent::ClaudeCode);
    let mut st = Settings::default();
    st.source_roots.insert(id.clone(), "/old/root".into());
    let (mut s, l, c) = setup(&st);
    s.tick(true);
    st.source_roots.insert(id, "/new/root".into());
    s.update_settings(&st);
    s.tick(true);
    let a = l.argvs();
    assert_eq!(a.len(), 1);
    assert!(a[0].contains(&"/old/root".to_string()) && !a[0].contains(&"/new/root".to_string()));
    l.0.lock().unwrap().exit_now = true;
    s.tick(true);
    for _ in 0..70 {
        c.advance(secs(1));
        s.tick(true);
    }
    let a = l.argvs();
    assert!(a.len() >= 2);
    let i = a[1].iter().position(|x| x == "--roots").unwrap();
    assert_eq!(a[1][i + 1], "/new/root");
}

struct Reg;
impl LxssRegistry for Reg {
    fn list(&self) -> io::Result<Vec<Distro>> {
        Ok(vec![Distro { name: "Ubuntu".into(), version: 2 }])
    }
}
struct Probe;
impl RunningProbe for Probe {
    fn running(&self) -> io::Result<Vec<String>> {
        Ok(vec!["Ubuntu".into()])
    }
}
struct NoLaunch;
impl lens_core::distro::Launcher for NoLaunch {
    fn launch(&self, _: &str) {}
}

#[test]
fn f32_disabled_distro_applies_at_poll_tick() {
    let (mut s, _l, c) = setup(&Settings::default());
    let (reg, probe, nl) = (Reg, Probe, NoLaunch);
    let mut m = DistroMonitor::new(&reg, &probe, &nl, &*c);
    s.tick_with(m.poll(&[]));
    assert!(!s.is_stopped());
    let disabled = vec!["Ubuntu".to_string()];
    c.advance(secs(59));
    s.tick_with(m.poll(&disabled));
    assert!(!s.is_stopped(), "still monitored at +59 s");
    c.advance(secs(1));
    s.tick_with(m.poll(&disabled));
    assert!(s.is_stopped(), "stopped at +60 s");
    assert!(s.health().is_none());
}

fn delivered_with_offset(behind_ms: i64) -> DateTime<Utc> {
    let (mut s, l, _c) = setup(&Settings::default());
    let helper_now = t0() - CDuration::milliseconds(behind_ms);
    let last = Utc.with_ymd_and_hms(2026, 1, 1, 11, 59, 50).unwrap();
    l.push_output(format!("{}\n{}\n", hello(helper_now), upsert("t", Some(last))));
    s.tick(true);
    s.tick(true);
    let ss = s.sessions();
    DateTime::parse_from_rfc3339(ss[0].last_activity_at.as_deref().unwrap()).unwrap().with_timezone(&Utc)
}

#[test]
fn clock_offset_applied_to_last_activity() {
    let ts = |h, m, s| Utc.with_ymd_and_hms(2026, 1, 1, h, m, s).unwrap();
    assert_eq!(delivered_with_offset(8000), ts(11, 59, 58));
    assert_eq!(delivered_with_offset(5000), ts(11, 59, 50));
    assert_eq!(delivered_with_offset(-5000), ts(11, 59, 50));
}
