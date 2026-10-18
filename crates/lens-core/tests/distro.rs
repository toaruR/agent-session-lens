use std::cell::Cell;
use std::io;
use std::time::{Duration, SystemTime};

use lens_core::clock::FakeClock;
use lens_core::distro::*;
use lens_core::model::v5::SourceState;

struct FakeReg(Vec<Distro>);
impl LxssRegistry for FakeReg {
    fn list(&self) -> io::Result<Vec<Distro>> {
        Ok(self.0.clone())
    }
}
struct FakeProbe {
    running: Vec<String>,
    calls: Cell<u32>,
}
impl RunningProbe for FakeProbe {
    fn running(&self) -> io::Result<Vec<String>> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.running.clone())
    }
}
#[derive(Default)]
struct FakeLauncher(Cell<u32>);
impl Launcher for FakeLauncher {
    fn launch(&self, _: &str) {
        self.0.set(self.0.get() + 1);
    }
}

fn ubuntu(v: u32) -> FakeReg {
    FakeReg(vec![Distro { name: "Ubuntu".into(), version: v }])
}
fn probe(running: &[&str]) -> FakeProbe {
    FakeProbe { running: running.iter().map(|s| s.to_string()).collect(), calls: Cell::new(0) }
}
fn clock() -> FakeClock {
    FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))
}

fn utf16le(s: &str, bom: bool) -> Vec<u8> {
    let mut v = Vec::new();
    if bom {
        v.extend([0xFF, 0xFE]);
    }
    for u in s.encode_utf16() {
        v.extend(u.to_le_bytes());
    }
    v
}

#[test]
fn version1_distro_is_unsupported_and_not_started() {
    let (reg, p, l, c) = (ubuntu(1), probe(&["Ubuntu"]), FakeLauncher::default(), clock());
    let mut m = DistroMonitor::new(&reg, &p, &l, &c);
    let st = m.poll(&[]).to_vec();
    assert_eq!(st[0].state, DistroState::Unsupported);
    assert_eq!(st[0].source_health().unwrap().state, SourceState::Unsupported);
    assert_eq!(l.0.get(), 0);
}

#[test]
fn utf16le_running_list_decoded() {
    let want = vec!["Ubuntu".to_string(), "Debian".to_string()];
    assert_eq!(decode_running_list(&utf16le("Ubuntu\r\nDebian\r\n", false)), want);
    assert_eq!(decode_running_list(&utf16le("Ubuntu\r\nDebian\r\n", true)), want);
}

#[test]
fn poll_period_59_vs_60() {
    let (reg, p, l, c) = (ubuntu(2), probe(&["Ubuntu"]), FakeLauncher::default(), clock());
    let mut m = DistroMonitor::new(&reg, &p, &l, &c);
    m.poll(&[]);
    assert_eq!(p.calls.get(), 1);
    c.advance(Duration::from_secs(59));
    m.poll(&[]);
    assert_eq!(p.calls.get(), 1);
    c.advance(Duration::from_secs(1));
    m.poll(&[]);
    assert_eq!(p.calls.get(), 2);
}

#[test]
fn disabled_distro_never_probed() {
    let (reg, p, l, c) = (ubuntu(2), probe(&["Ubuntu"]), FakeLauncher::default(), clock());
    let mut m = DistroMonitor::new(&reg, &p, &l, &c);
    let dis = vec!["Ubuntu".to_string()];
    for _ in 0..120 {
        m.poll(&dis);
        c.advance(Duration::from_secs(1));
    }
    assert_eq!(p.calls.get(), 0);
    assert_eq!(l.0.get(), 0);
}

#[test]
fn stopped_distro_not_started() {
    let (reg, p, l, c) = (ubuntu(2), probe(&[]), FakeLauncher::default(), clock());
    let mut m = DistroMonitor::new(&reg, &p, &l, &c);
    let st = m.poll(&[]).to_vec();
    assert_eq!(st[0].state, DistroState::Stopped);
    assert_eq!(st[0].source_health().unwrap().state, SourceState::DistroStopped);
    assert_eq!(l.0.get(), 0);
}
