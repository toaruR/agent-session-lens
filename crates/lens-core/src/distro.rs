//! WSL distro discovery (registry, no WSL wake-up) and running-state polling.
//!
//! Only `Version=2` distros are monitored; others are `unsupported`. Stopped distros
//! are never started; distros in `disabledDistros` are never probed.

use std::io;
use std::time::{Duration, SystemTime};

use crate::clock::Clock;
use crate::consts::DISTRO_POLL_SECS;
use crate::model::v5::{Origin, OriginKind, SourceHealth, SourceState};
use crate::process;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    pub name: String,
    pub version: u32,
}

/// Reads registered distros from `HKCU\...\Lxss\{GUID}` without waking WSL.
pub trait LxssRegistry {
    fn list(&self) -> io::Result<Vec<Distro>>;
}

/// Returns the names of currently running distros.
pub trait RunningProbe {
    fn running(&self) -> io::Result<Vec<String>>;
}

/// Starts the helper for a running distro (implemented by a later task).
pub trait Launcher {
    fn launch(&self, distro: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistroState {
    Running,
    Stopped,
    Unsupported,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroStatus {
    pub name: String,
    pub state: DistroState,
}

impl DistroStatus {
    /// SourceHealth for non-running distros; `None` for running ones (helper reports itself).
    pub fn source_health(&self) -> Option<SourceHealth> {
        let state = match self.state {
            DistroState::Running | DistroState::Disabled => return None,
            DistroState::Stopped => SourceState::DistroStopped,
            DistroState::Unsupported => SourceState::Unsupported,
        };
        Some(SourceHealth {
            source_id: format!("wsl:{}", self.name),
            agent: None,
            origin: Origin { kind: OriginKind::Wsl, distro: Some(self.name.clone()) },
            root_path: String::new(),
            state,
            last_scan_at: None,
            files_tracked: 0,
            sessions_total: 0,
            partial_sessions: 0,
            skipped_old_files: 0,
            top_diagnostics: Vec::new(),
        })
    }
}

/// Decode `wsl.exe --list --running --quiet` output (UTF-16LE, optional BOM, CRLF).
pub fn decode_running_list(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_prefix(&[0xFF, 0xFE][..]).unwrap_or(bytes);
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Real probe: runs `wsl.exe --list --running --quiet` through process.rs.
pub struct WslExeProbe;

impl RunningProbe for WslExeProbe {
    fn running(&self) -> io::Result<Vec<String>> {
        let mut sp = process::spawn("wsl.exe", ["--list", "--running", "--quiet"])?;
        let mut out = Vec::new();
        if let Some(mut so) = sp.child.stdout.take() {
            io::Read::read_to_end(&mut so, &mut out)?;
        }
        let _ = sp.child.wait();
        Ok(decode_running_list(&out))
    }
}

/// Polls distro state on a DISTRO_POLL_SECS schedule.
pub struct DistroMonitor<'a> {
    registry: &'a dyn LxssRegistry,
    probe: &'a dyn RunningProbe,
    launcher: &'a dyn Launcher,
    clock: &'a dyn Clock,
    last_poll: Option<SystemTime>,
    cached: Vec<DistroStatus>,
}

impl<'a> DistroMonitor<'a> {
    pub fn new(
        registry: &'a dyn LxssRegistry,
        probe: &'a dyn RunningProbe,
        launcher: &'a dyn Launcher,
        clock: &'a dyn Clock,
    ) -> Self {
        Self { registry, probe, launcher, clock, last_poll: None, cached: Vec::new() }
    }

    /// Returns current statuses; re-evaluates only when the poll period has elapsed.
    pub fn poll(&mut self, disabled: &[String]) -> &[DistroStatus] {
        let now = self.clock.now();
        let due = match self.last_poll {
            None => true,
            Some(t) => now.duration_since(t).unwrap_or_default() >= Duration::from_secs(DISTRO_POLL_SECS),
        };
        if due {
            self.last_poll = Some(now);
            self.cached = self.evaluate(disabled);
        }
        &self.cached
    }

    fn evaluate(&self, disabled: &[String]) -> Vec<DistroStatus> {
        let distros = self.registry.list().unwrap_or_default();
        let candidate = |d: &Distro| d.version == 2 && !disabled.contains(&d.name);
        let running = if distros.iter().any(candidate) {
            self.probe.running().unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut out = Vec::new();
        for d in &distros {
            let state = if disabled.contains(&d.name) {
                DistroState::Disabled
            } else if d.version != 2 {
                DistroState::Unsupported
            } else if running.contains(&d.name) {
                self.launcher.launch(&d.name);
                DistroState::Running
            } else {
                DistroState::Stopped
            };
            out.push(DistroStatus { name: d.name.clone(), state });
        }
        out
    }
}

/// Real registry reader (Windows) / empty list elsewhere.
pub struct SystemLxssRegistry;

#[cfg(not(windows))]
impl LxssRegistry for SystemLxssRegistry {
    fn list(&self) -> io::Result<Vec<Distro>> {
        Ok(Vec::new())
    }
}

#[cfg(windows)]
impl LxssRegistry for SystemLxssRegistry {
    fn list(&self) -> io::Result<Vec<Distro>> {
        use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
        use windows_sys::Win32::System::Registry::*;

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        unsafe fn read_value(key: HKEY, name: &str, flags: u32) -> Option<Vec<u8>> {
            let n = wide(name);
            let mut size: u32 = 0;
            if RegGetValueW(key, std::ptr::null(), n.as_ptr(), flags, std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            if RegGetValueW(
                key,
                std::ptr::null(),
                n.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                &mut size,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            buf.truncate(size as usize);
            Some(buf)
        }

        let mut out = Vec::new();
        unsafe {
            let mut root: HKEY = std::ptr::null_mut();
            let path = wide(r"Software\Microsoft\Windows\CurrentVersion\Lxss");
            if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut root) != ERROR_SUCCESS {
                return Ok(out);
            }
            let mut idx = 0u32;
            loop {
                let mut name = [0u16; 256];
                let mut len = name.len() as u32;
                let rc = RegEnumKeyExW(
                    root,
                    idx,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if rc == ERROR_NO_MORE_ITEMS {
                    break;
                }
                idx += 1;
                if rc != ERROR_SUCCESS {
                    continue;
                }
                let mut sub: HKEY = std::ptr::null_mut();
                if RegOpenKeyExW(root, name.as_ptr(), 0, KEY_READ, &mut sub) != ERROR_SUCCESS {
                    continue;
                }
                let dn = read_value(sub, "DistributionName", RRF_RT_REG_SZ);
                let ver = read_value(sub, "Version", RRF_RT_REG_DWORD);
                RegCloseKey(sub);
                if let (Some(dn), Some(ver)) = (dn, ver) {
                    let units: Vec<u16> = dn.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    let s = String::from_utf16_lossy(&units);
                    let s = s.trim_end_matches('\0').to_string();
                    if ver.len() >= 4 && !s.is_empty() {
                        out.push(Distro { name: s, version: u32::from_le_bytes([ver[0], ver[1], ver[2], ver[3]]) });
                    }
                }
            }
            RegCloseKey(root);
        }
        Ok(out)
    }
}
