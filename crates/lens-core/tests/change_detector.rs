use lens_core::change_detector::{backoff_secs, Backoff, ChangeDetector};
use lens_core::clock::FakeClock;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn setup() -> (tempfile::TempDir, Arc<FakeClock>) {
    (tempfile::tempdir().unwrap(), Arc::new(FakeClock::new(SystemTime::now())))
}

#[test]
fn hot_poll() {
    let (dir, clock) = setup();
    let f = dir.path().join("a.jsonl");
    std::fs::write(&f, "x\n").unwrap();
    let mut d = ChangeDetector::new(dir.path(), clock.clone());
    std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"more\n").unwrap();
    clock.advance(Duration::from_secs(1));
    let r = d.tick();
    assert!(r.hot_changed.is_empty() && !r.reconciled);
    clock.advance(Duration::from_secs(1));
    let r = d.tick();
    assert_eq!(r.hot_changed, vec![f]);
    assert!(d.tick().hot_changed.is_empty());
}

#[test]
fn reconcile() {
    let (dir, clock) = setup();
    let mut d = ChangeDetector::new(dir.path(), clock.clone());
    let f = dir.path().join("new.jsonl");
    std::fs::write(&f, "x\n").unwrap();
    clock.advance(Duration::from_secs(59));
    let r = d.tick();
    assert!(r.new_files.is_empty());
    clock.advance(Duration::from_secs(1));
    let r = d.tick();
    assert!(r.reconciled);
    assert_eq!(r.new_files, vec![f]);
}

#[test]
fn overflow() {
    let (dir, clock) = setup();
    let mut d = ChangeDetector::new(dir.path(), clock.clone());
    assert_eq!(d.reconcile_count(), 0);
    d.feed_overflow();
    let r = d.tick();
    assert!(r.reconciled);
    assert_eq!(d.reconcile_count(), 1);
    assert!(!d.tick().reconciled);
    assert_eq!(d.reconcile_count(), 1);
}

#[test]
fn backoff() {
    let got: Vec<u64> = (1..=8).map(backoff_secs).collect();
    assert_eq!(got, vec![1, 2, 4, 8, 16, 32, 60, 60]);
    let mut b = Backoff::default();
    for _ in 0..5 {
        b.on_failure();
    }
    b.on_success();
    assert_eq!(b.on_failure(), 1);
    assert_eq!(backoff_secs(u32::MAX), 60);
}

#[test]
fn debounce() {
    let (dir, clock) = setup();
    let mut d = ChangeDetector::new(dir.path(), clock.clone());
    let p = PathBuf::from("x");
    let mut batches = 0;
    for _ in 0..5 {
        d.feed_event(p.clone());
        batches += d.tick().notify_batches.len();
        clock.advance(Duration::from_millis(39));
    }
    clock.advance(Duration::from_millis(4)); // 199 ms since first event
    batches += d.tick().notify_batches.len();
    assert_eq!(batches, 0);
    clock.advance(Duration::from_millis(1)); // 200 ms
    d.feed_event(p.clone()); // closes first window, starts a new one
    batches += d.tick().notify_batches.len();
    assert_eq!(batches, 1);
    clock.advance(Duration::from_millis(200));
    batches += d.tick().notify_batches.len();
    assert_eq!(batches, 2);
}
