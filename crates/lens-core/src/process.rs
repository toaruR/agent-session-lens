//! Child process spawn wrapper. The only place that may call `Command::new`.
//!
//! Children get `Stdio::null()` for stdin (so they never block on or inherit the
//! parent's pipe) and, on Windows, are placed in a Job Object with
//! KILL_ON_JOB_CLOSE so they die with the parent. Elsewhere the job is `None`.

use std::ffi::OsStr;
use std::io;
use std::process::{Child, Command, Stdio};

/// Owned Job Object handle (Windows). Closing it kills the contained processes.
#[cfg(windows)]
pub struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for JobHandle {}

#[cfg(windows)]
impl JobHandle {
    pub fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0
    }

    fn create() -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = JobHandle(h);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Placeholder on non-Windows: no job is ever created.
#[cfg(not(windows))]
pub struct JobHandle(());

/// A spawned child plus its Job Object (Windows only).
pub struct SpawnedChild {
    pub child: Child,
    pub job: Option<JobHandle>,
}

impl SpawnedChild {
    /// Close the job handle; on Windows this terminates the child.
    pub fn close_job(&mut self) {
        self.job = None;
    }
}

/// Spawn `program` with null stdin, piped stdout/stderr, no console window.
pub fn spawn<I, S>(program: impl AsRef<OsStr>, args: I) -> io::Result<SpawnedChild>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    spawn_with(program, args, |_| {})
}

/// Like [`spawn`], with a hook to customise the command (env, cwd) before launch.
pub fn spawn_with<I, S>(
    program: impl AsRef<OsStr>,
    args: I,
    configure: impl FnOnce(&mut Command),
) -> io::Result<SpawnedChild>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    configure(&mut cmd);
    let mut child = cmd.spawn()?;
    #[cfg(windows)]
    let job = {
        use std::os::windows::io::AsRawHandle;
        let job = JobHandle::create().and_then(|j| {
            let ok = unsafe {
                windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                    j.raw(),
                    child.as_raw_handle() as _,
                )
            };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(j)
            }
        });
        match job {
            Ok(j) => Some(j),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
    };
    #[cfg(not(windows))]
    let job = None;
    Ok(SpawnedChild { child, job })
}
