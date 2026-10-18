//! WSL helper supervisor core (architecture-v5 section 6): launch, heartbeat watchdog,
//! backoff restart and the stopped-distro state. Time comes from the injected `Clock`;
//! the process side is behind `Launcher` so tests never start a real `wsl.exe`.
//!
//! `tick` is idempotent: calling it on every re-evaluation while the distro is running
//! never launches a second helper (duplicate launches are suppressed).

use std::io::{self, Read};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::change_detector::Backoff;
use crate::clock::Clock;
use crate::consts::HB_TIMEOUT_SECS;
use crate::clock_offset::ClockOffset;
use crate::distro::{DistroState, DistroStatus};
use crate::model::v5::{wsl_source_id, SourceHealth, SourceState, UnifiedSession};
use crate::redact::{sanitize, Field};
use chrono::{DateTime, Utc};
use crate::ndjson::Receiver;
use crate::model::{Agent, Diagnostic};
use crate::process::{self, SpawnedChild};
use crate::settings::Settings;

pub const HELPER_NOT_FOUND: &str = "helper_not_found";
pub const LAUNCH_FAILED: &str = "launch_failed";
pub const USAGE_ERROR: &str = "usage_error";
pub const VERSION_MISMATCH: &str = "helper_version_mismatch";

/// Exit codes of section 6.2.
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_PROTOCOL: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    /// The helper binary (the `--exec` target) does not exist.
    NotFound,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperExit {
    Code(i32),
    /// The exit was identified as a missing `--exec` target.
    NotFound,
}

/// A running helper child.
pub trait HelperProcess: Send {
    /// `Some` once the process has exited.
    fn poll_exit(&mut self) -> Option<HelperExit>;
    fn kill(&mut self);
    /// Bytes the helper wrote to stdout since the last call (NDJSON). Default: none.
    fn read_stdout(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

/// Starts `argv` (argv[0] is the program). Real impl: [`WslLauncher`].
pub trait Launcher: Send + Sync {
    fn launch(&self, argv: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError>;
}

/// `wsl.exe -d <distro> --exec <helper> serve --protocol 1 [--roots <path>...]`
pub fn launch_argv(distro: &str, settings: &Settings) -> Vec<String> {
    let mut v: Vec<String> = ["wsl.exe", "-d", distro, "--exec", settings.effective_helper_path(), "serve", "--protocol", "1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let roots: Vec<&String> = [Agent::ClaudeCode, Agent::Codex]
        .into_iter()
        .filter_map(|a| settings.source_roots.get(&wsl_source_id(distro, a)))
        .collect();
    if !roots.is_empty() {
        v.push("--roots".into());
        v.extend(roots.into_iter().cloned());
    }
    v
}

/// Real launcher through process.rs. How wsl.exe reports a missing `--exec` target is not
/// specified by the design: a spawn `NotFound` or an exit whose stderr names a missing
/// file is mapped to `NotFound` here. Only checked manually on Windows.
pub struct WslLauncher;

struct RealProcess {
    sp: SpawnedChild,
}

impl HelperProcess for RealProcess {
    fn poll_exit(&mut self) -> Option<HelperExit> {
        let st = self.sp.child.try_wait().ok().flatten()?;
        let mut err = String::new();
        if let Some(se) = self.sp.child.stderr.take() {
            let _ = se.take(4096).read_to_string(&mut err);
        }
        let lower = err.to_ascii_lowercase();
        if lower.contains("no such file") || lower.contains("not found") || lower.contains("enoent") {
            return Some(HelperExit::NotFound);
        }
        Some(HelperExit::Code(st.code().unwrap_or(-1)))
    }

    fn kill(&mut self) {
        let _ = self.sp.child.kill();
        self.sp.close_job();
        let _ = self.sp.child.wait();
    }
}

impl Launcher for WslLauncher {
    fn launch(&self, argv: &[String]) -> Result<Box<dyn HelperProcess>, LaunchError> {
        let (prog, args) = argv.split_first().ok_or(LaunchError::Other)?;
        match process::spawn(prog, args) {
            Ok(sp) => Ok(Box::new(RealProcess { sp })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(LaunchError::NotFound),
            Err(_) => Err(LaunchError::Other),
        }
    }
}

fn system_ms(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

/// Supervises the helper of one distro.
pub struct Supervisor {
    distro: String,
    argv: Vec<String>,
    /// Latest settings snapshot; helperPath / sourceRoots are read only at the next launch.
    settings: Settings,
    offset: ClockOffset,
    disabled: bool,
    launcher: Arc<dyn Launcher>,
    clock: Arc<dyn Clock>,
    backoff: Backoff,
    child: Option<Box<dyn HelperProcess>>,
    last_alive: SystemTime,
    next_launch_at: Option<SystemTime>,
    stopped: bool,
    unavailable: Option<&'static str>,
    /// Terminal state (usage error / version mismatch): never restart.
    fatal: Option<&'static str>,
    receiver: Receiver,
    restarts: u64,
    launches: u64,
    generation: u64,
    waits: Vec<u64>,
}

impl Supervisor {
    pub fn new(distro: &str, settings: &Settings, launcher: Arc<dyn Launcher>, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now();
        Self {
            distro: distro.to_string(),
            argv: launch_argv(distro, settings),
            settings: settings.clone(),
            offset: ClockOffset::new(),
            disabled: false,
            launcher,
            clock,
            backoff: Backoff::default(),
            child: None,
            last_alive: now,
            next_launch_at: None,
            stopped: false,
            unavailable: None,
            fatal: None,
            receiver: Receiver::new(),
            restarts: 0,
            launches: 0,
            generation: 0,
            waits: Vec::new(),
        }
    }

    /// Drive the state machine. `distro_running` comes from the distro monitor.
    pub fn tick(&mut self, distro_running: bool) {
        if !distro_running {
            if let Some(mut c) = self.child.take() {
                c.kill();
            }
            self.next_launch_at = None;
            self.stopped = true;
            return;
        }
        self.stopped = false;
        if self.fatal.is_some() {
            return;
        }
        let now = self.clock.now();
        self.drain_stdout();
        if self.receiver.is_stopped() {
            // hello.protocol != PROTOCOL_VERSION: stop, no restart.
            if let Some(mut c) = self.child.take() {
                c.kill();
            }
            self.fatal = Some(VERSION_MISMATCH);
            return;
        }
        if let Some(c) = self.child.as_mut() {
            if let Some(exit) = c.poll_exit() {
                self.child = None;
                // Output written just before exiting still counts.
                match exit {
                    HelperExit::Code(EXIT_USAGE) => {
                        self.fatal = Some(USAGE_ERROR);
                        return;
                    }
                    HelperExit::Code(EXIT_PROTOCOL) => {
                        self.fatal = Some(VERSION_MISMATCH);
                        return;
                    }
                    HelperExit::NotFound => self.unavailable = Some(HELPER_NOT_FOUND),
                    HelperExit::Code(_) => {}
                }
                self.schedule_restart(now, true);
            } else if now.duration_since(self.last_alive).unwrap_or_default() >= Duration::from_secs(HB_TIMEOUT_SECS) {
                if let Some(mut c) = self.child.take() {
                    c.kill();
                }
                self.schedule_restart(now, true);
            }
        }
        if self.child.is_none() && self.next_launch_at.is_none_or(|t| now >= t) {
            self.launch(now);
        }
    }

    /// Store new settings. The running helper is untouched: `helperPath` and `sourceRoots`
    /// take effect at the next launch; `disabledDistros` is applied by the caller at the
    /// next `DistroMonitor` poll (see [`Supervisor::tick_with`]).
    pub fn update_settings(&mut self, settings: &Settings) {
        self.settings = settings.clone();
    }

    /// Tick from the distro monitor's statuses (this distro's entry; absent = stopped).
    pub fn tick_with(&mut self, statuses: &[DistroStatus]) {
        let st = statuses.iter().find(|d| d.name == self.distro).map(|d| d.state);
        self.disabled = st == Some(DistroState::Disabled);
        self.tick(st == Some(DistroState::Running));
    }

    /// Sessions of the current helper generation as delivered to the Aggregator / IPC layer:
    /// re-sanitized and with `lastActivityAt` corrected by the clock offset.
    pub fn sessions(&self) -> Vec<UnifiedSession> {
        let now_ms = system_ms(self.clock.now());
        self.receiver
            .sessions()
            .values()
            .map(|s| {
                let mut s = s.clone();
                let t = &mut s.current_task;
                for x in [&mut t.title, &mut t.last_user_message, &mut t.last_assistant_message].into_iter().flatten() {
                    *x = sanitize(x, Field::Text);
                }
                if let Some(a) = &mut t.active_tool {
                    a.input = sanitize(&a.input, Field::Text);
                }
                if let Some(ts) = s.last_activity_at.as_deref().and_then(|x| DateTime::parse_from_rfc3339(x).ok()) {
                    let c = self.offset.correct(ts.timestamp_millis(), now_ms);
                    if let Some(d) = DateTime::<Utc>::from_timestamp_millis(c.t_ms) {
                        s.last_activity_at = Some(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                    }
                }
                s
            })
            .collect()
    }

    fn drain_stdout(&mut self) {
        let Some(c) = self.child.as_mut() else { return };
        let bytes = c.read_stdout();
        if bytes.is_empty() {
            return;
        }
        let outs = self.receiver.feed(&bytes);
        let recv_ms = system_ms(self.clock.now());
        for o in &outs {
            let helper_now = match o {
                crate::ndjson::Outcome::Accepted(crate::ndjson::Accepted::Hello) => self.receiver.hello().map(|h| h.now.clone()),
                crate::ndjson::Outcome::Accepted(crate::ndjson::Accepted::Heartbeat) => self.receiver.last_heartbeat().map(str::to_string),
                _ => None,
            };
            if let Some(t) = helper_now.and_then(|x| DateTime::parse_from_rfc3339(&x).ok()) {
                self.offset.record(recv_ms, t.timestamp_millis());
            }
        }
        if outs.iter().any(|o| matches!(o, crate::ndjson::Outcome::Accepted(_))) {
            self.on_alive();
        }
    }

    /// Receiver of the current helper generation (hello, sessions, source health).
    pub fn receiver(&self) -> &Receiver {
        &self.receiver
    }

    fn launch(&mut self, now: SystemTime) {
        self.next_launch_at = None;
        // Settings snapshot is read here, at launch time.
        self.argv = launch_argv(&self.distro, &self.settings);
        match self.launcher.launch(&self.argv) {
            Ok(c) => {
                self.child = Some(c);
                self.receiver = Receiver::new();
                self.offset = ClockOffset::new();
                self.launches += 1;
                self.generation += 1;
                self.last_alive = now;
            }
            Err(e) => {
                self.unavailable = Some(if e == LaunchError::NotFound { HELPER_NOT_FOUND } else { LAUNCH_FAILED });
                self.schedule_restart(now, false);
            }
        }
    }

    fn schedule_restart(&mut self, now: SystemTime, count_restart: bool) {
        let wait = self.backoff.on_failure();
        self.waits.push(wait);
        if count_restart {
            self.restarts += 1;
        }
        self.next_launch_at = Some(now + Duration::from_secs(wait));
    }

    /// Any hello/heartbeat/message from the helper: resets the watchdog and the backoff.
    pub fn on_alive(&mut self) {
        self.last_alive = self.clock.now();
        self.backoff.on_success();
        self.unavailable = None;
    }

    /// Restarts triggered by heartbeat silence or process exit.
    pub fn restarts(&self) -> u64 {
        self.restarts
    }
    /// Successful `wsl.exe -d` launches.
    pub fn launches(&self) -> u64 {
        self.launches
    }
    /// Increments on every successful launch; a new value means state must be rebuilt and rescanned.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Every backoff wait scheduled so far, in seconds.
    pub fn waits(&self) -> &[u64] {
        &self.waits
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
    /// Feed to `Aggregator::set_distro_available`.
    pub fn origin_available(&self) -> bool {
        !self.stopped
    }

    /// Supervisor-owned SourceHealth: `distro_stopped` or `helper_unavailable`; `None` otherwise.
    pub fn health(&self) -> Option<SourceHealth> {
        let mut h = DistroStatus { name: self.distro.clone(), state: DistroState::Stopped }.source_health()?;
        if self.disabled {
            return None;
        }
        if self.stopped {
            return Some(h);
        }
        if let Some(code) = self.fatal {
            h.state = if code == VERSION_MISMATCH { SourceState::HelperVersionMismatch } else { SourceState::HelperUnavailable };
            h.top_diagnostics = vec![Diagnostic { code: code.into(), count: 1 }];
            return Some(h);
        }
        let code = self.unavailable?;
        h.state = SourceState::HelperUnavailable;
        h.top_diagnostics = vec![Diagnostic { code: code.into(), count: 1 }];
        Some(h)
    }
}
