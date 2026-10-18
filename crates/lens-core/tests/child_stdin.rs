use lens_core::clock::{Clock, FakeClock};
use lens_core::process::{spawn_with, SpawnedChild};
use std::io::{BufRead, BufReader, Read};
use std::time::{Duration, Instant, SystemTime};

const ENV: &str = "LENS_CHILD_STDIN_HELPER";

/// Helper entry: runs only when re-invoked as a child.
#[test]
fn child_helper() {
    if std::env::var(ENV).is_err() {
        return;
    }
    println!("ready");
    if std::env::var(ENV).as_deref() == Ok("sleep") {
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    let mut buf = Vec::new();
    let n = std::io::stdin().read_to_end(&mut buf).unwrap();
    println!("read {n}");
}

fn spawn_helper() -> SpawnedChild {
    spawn_helper_mode("1")
}

fn spawn_helper_mode(mode: &'static str) -> SpawnedChild {
    let exe = std::env::current_exe().unwrap();
    spawn_with(
        exe,
        ["--exact", "child_helper", "--nocapture", "--test-threads=1"],
        |c| {
            c.env(ENV, mode);
        },
    )
    .unwrap()
}

#[test]
fn f9_child_stdin_null_exits_zero_read_zero() {
    // Parent-side pipe that stays open and is never written to.
    let (_r, _w) = std::io::pipe().unwrap();
    let mut sc = spawn_helper();
    let mut out = BufReader::new(sc.child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(out.read_line(&mut line).unwrap() > 0, "no ready");
        if line.trim().ends_with("ready") {
            break;
        }
    }
    let clock = FakeClock::new(SystemTime::UNIX_EPOCH);
    let t0 = clock.now();
    clock.advance(Duration::from_secs(10));
    assert_eq!(clock.now().duration_since(t0).unwrap(), Duration::from_secs(10));
    // Child must already be done; real-time guard is generous but bounded.
    let start = Instant::now();
    let status = loop {
        if let Some(s) = sc.child.try_wait().unwrap() {
            break s;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "child still running at +10s");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(0));
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    assert!(rest.lines().any(|l| l.trim() == "read 0"), "output: {rest}");
}

#[test]
fn job_object_contains_child_windows() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        let mut sc = spawn_helper_mode("sleep");
        let job = sc.job.as_ref().expect("job");
        let mut result = 0i32;
        let ok = unsafe { IsProcessInJob(sc.child.as_raw_handle() as _, job.raw(), &mut result) };
        assert!(ok != 0 && result != 0, "child not in job");
        assert!(sc.child.try_wait().unwrap().is_none(), "child should be running");
        sc.close_job();
        let start = Instant::now();
        loop {
            if sc.child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "child not terminated");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn spawn_without_job_on_non_windows() {
    #[cfg(not(windows))]
    {
        let mut sc = spawn_helper();
        assert!(sc.job.is_none());
        let _ = sc.child.wait();
    }
}
