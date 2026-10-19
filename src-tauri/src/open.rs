//! Shift/Ctrl+click on a project: reveal its folder, or bring windows working on it to the front.
//!
//! Editors (VS Code / Cursor / Antigravity IDE) are matched by window title, the same way as
//! vscode-selector. Agent CLIs (claude / codex / agy) are matched by process cwd and resolved to
//! the nearest ancestor process that owns a top-level window (terminal, desktop app).

use crate::title_settings::EDITOR_DIRS;
use lens_core::project::normalize;

/// The project as the window matchers see it.
pub struct Target {
    /// Comparison key (`lens_core::project::normalize`).
    pub key: String,
    pub name: String,
    /// `Some` for a project inside WSL.
    pub distro: Option<String>,
    /// Path openable by Explorer.
    pub path: String,
}

const PATH_OPEN: char = '⟨';
const PATH_CLOSE: char = '⟩';

/// `path` (an editor root or a process cwd) is the project or a folder inside it.
fn path_matches(path: &str, t: &Target) -> bool {
    let Some(n) = normalize(path, t.distro.as_deref()) else { return false };
    let sep = if t.distro.is_some() { '/' } else { '\\' };
    n.key == t.key || n.key.strip_prefix(&t.key).is_some_and(|rest| rest.starts_with(sep))
}

/// Parts of an editor window title, e.g. `● main.rs - repo [WSL: Ubuntu] ⟨/home/u/repo⟩ - Visual Studio Code`.
struct EditorTitle {
    root_name: String,
    /// Remote label such as `WSL: Ubuntu`.
    label: Option<String>,
    /// From the `⟨${rootPath}⟩` marker that vscode-selector can add to window.title.
    /// `None` when absent, empty or abbreviated (`~` in WSL), so only the name can be compared.
    root_path: Option<String>,
}

fn parse_editor_title(title: &str) -> Option<EditorTitle> {
    let mut title = title.to_string();
    let mut root_path = None;
    if let (Some(a), Some(b)) = (title.find(PATH_OPEN), title.find(PATH_CLOSE)) {
        if a < b {
            root_path = Some(title[a + PATH_OPEN.len_utf8()..b].trim().to_string());
            let start = if title[..a].ends_with(' ') { a - 1 } else { a };
            title.replace_range(start..b + PATH_CLOSE.len_utf8(), "");
        }
    }
    let rest = EDITOR_DIRS.iter().find_map(|(app, _)| title.strip_suffix(&format!(" - {app}")))?;
    let mut root = rest.rsplit(" - ").next().unwrap_or(rest);
    let mut label = None;
    if let Some(i) = root.rfind(" [").filter(|_| root.ends_with(']')) {
        label = Some(root[i + 2..root.len() - 1].to_string());
        root = &root[..i];
    }
    let root = root.strip_suffix(" (Workspace)").unwrap_or(root);
    let root = root.strip_prefix("[Administrator] ").unwrap_or(root);
    let root = root.strip_prefix("● ").or_else(|| root.strip_prefix("* ")).unwrap_or(root);
    let root_path = root_path.filter(|p| !p.is_empty() && !p.starts_with('~'));
    Some(EditorTitle { root_name: root.to_string(), label, root_path })
}

fn origin_matches(e: &EditorTitle, t: &Target) -> bool {
    match (&t.distro, e.label.as_deref()) {
        (None, None) => true,
        (Some(d), Some(l)) => l.strip_prefix("WSL: ").is_some_and(|x| x.eq_ignore_ascii_case(d)),
        _ => false,
    }
}

/// The editor has the project (or a folder inside it) open.
pub fn editor_title_matches(title: &str, t: &Target) -> bool {
    let Some(e) = parse_editor_title(title).filter(|e| origin_matches(e, t)) else { return false };
    match &e.root_path {
        Some(p) => path_matches(p, t),
        None => e.root_name.eq_ignore_ascii_case(&t.name),
    }
}

/// The editor has a parent folder of the project open: the length of its key (deeper is longer).
/// Needs the path marker; a folder name alone cannot tell.
pub fn editor_ancestor_depth(title: &str, t: &Target) -> Option<usize> {
    let e = parse_editor_title(title).filter(|e| origin_matches(e, t))?;
    let root = normalize(e.root_path.as_deref()?, t.distro.as_deref())?.key;
    let sep = if t.distro.is_some() { '/' } else { '\\' };
    let rest = t.key.strip_prefix(&root)?;
    (!rest.is_empty() && (root.ends_with(sep) || rest.starts_with(sep))).then_some(root.len())
}

/// Settings folder of the editor whose title lacks the path marker, so it can only be matched by name.
pub fn editor_without_marker(title: &str) -> Option<&'static str> {
    if title.contains(PATH_OPEN) { return None; }
    EDITOR_DIRS.iter().find(|(app, _)| title.ends_with(&format!(" - {app}"))).map(|&(_, dir)| dir)
}

/// Agent CLI process whose cwd places it in the project.
pub fn agent_cwd_matches(cwd: &str, t: &Target) -> bool {
    path_matches(cwd, t)
}

#[cfg(windows)]
pub use win::{editors_without_marker, focus_project, reveal_project};

#[cfg(not(windows))]
pub fn editors_without_marker() -> Vec<&'static str> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn reveal_project(_: &Target) -> Result<(), String> {
    Err("この操作は Windows でのみ利用できます".into())
}

#[cfg(not(windows))]
pub fn focus_project(_: &Target) -> Result<usize, String> {
    Err("この操作は Windows でのみ利用できます".into())
}

#[cfg(windows)]
mod win {
    use super::{agent_cwd_matches, editor_ancestor_depth, editor_title_matches, editor_without_marker, Target};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, IsWow64Process, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const EDITOR_CLASS: &str = "Chrome_WidgetWin_1";
    const AGENT_EXES: [&str; 3] = ["claude.exe", "codex.exe", "agy.exe"];
    // Ancestors whose windows do not identify the agent: editors are matched by title instead,
    // and Explorer windows are unrelated folders.
    const STOP_EXES: [&str; 6] = ["code.exe", "code - insiders.exe", "vscodium.exe", "cursor.exe", "antigravity ide.exe", "explorer.exe"];
    const CONSOLE_HOSTS: [&str; 2] = ["conhost.exe", "openconsole.exe"];
    const MAX_ANCESTORS: usize = 32;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn reveal_project(t: &Target) -> Result<(), String> {
        let (op, file) = (wide("open"), wide(&t.path));
        let r = unsafe { ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
        if r as usize > 32 { Ok(()) } else { Err("フォルダを開けませんでした".into()) }
    }

    /// Brings every matching window to the front. Returns how many were found.
    pub fn focus_project(t: &Target) -> Result<usize, String> {
        let windows = top_windows();
        let mut found: Vec<HWND> = Vec::new();
        if t.distro.is_none() {
            // WSL processes are not visible from Windows; only editors can be matched there.
            for hwnd in agent_windows(t, &windows) {
                if !found.contains(&hwnd) { found.push(hwnd); }
            }
        }
        // Editors last, so one of them ends up on top.
        let editors: Vec<&Win> = windows.iter().filter(|w| w.class == EDITOR_CLASS).collect();
        let mut matched: Vec<HWND> = editors.iter().filter(|w| editor_title_matches(&w.title, t)).map(|w| w.hwnd).collect();
        if matched.is_empty() {
            // None has the project itself open: fall back to the editors on its nearest parent folder.
            let depths: Vec<(HWND, usize)> = editors.iter().filter_map(|w| Some((w.hwnd, editor_ancestor_depth(&w.title, t)?))).collect();
            let deepest = depths.iter().map(|&(_, d)| d).max();
            matched = depths.into_iter().filter(|&(_, d)| Some(d) == deepest).map(|(h, _)| h).collect();
        }
        for hwnd in matched {
            if !found.contains(&hwnd) { found.push(hwnd); }
        }
        for &hwnd in &found {
            unsafe { activate(hwnd) };
        }
        Ok(found.len())
    }

    /// Settings folders of open editors whose window titles lack the path marker.
    pub fn editors_without_marker() -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for dir in top_windows().iter().filter(|w| w.class == EDITOR_CLASS).filter_map(|w| editor_without_marker(&w.title)) {
            if !out.contains(&dir) { out.push(dir); }
        }
        out
    }

    struct Win {
        hwnd: HWND,
        pid: u32,
        class: String,
        title: String,
    }

    struct Proc {
        parent: u32,
        exe: String,
    }

    fn agent_windows(t: &Target, windows: &[Win]) -> Vec<HWND> {
        let procs = processes();
        let mut out = Vec::new();
        for (&pid, p) in &procs {
            if !AGENT_EXES.contains(&p.exe.as_str()) { continue; }
            if !process_cwd(pid).is_some_and(|cwd| agent_cwd_matches(&cwd, t)) { continue; }
            if let Some(hwnd) = owner_window(pid, &procs, windows) { out.push(hwnd); }
        }
        out
    }

    /// First window owned by the process, an ancestor, or an ancestor's console host.
    fn owner_window(pid: u32, procs: &HashMap<u32, Proc>, windows: &[Win]) -> Option<HWND> {
        let window_of = |pid: u32| windows.iter().find(|w| w.pid == pid).map(|w| w.hwnd);
        let mut cur = pid;
        for _ in 0..MAX_ANCESTORS {
            let p = procs.get(&cur)?;
            if STOP_EXES.contains(&p.exe.as_str()) { return None; }
            if let Some(hwnd) = window_of(cur) { return Some(hwnd); }
            let host = procs.iter().find(|(&c, h)| h.parent == cur && CONSOLE_HOSTS.contains(&h.exe.as_str()) && window_of(c).is_some());
            if let Some((&c, _)) = host { return window_of(c); }
            if p.parent == 0 || p.parent == cur { return None; }
            cur = p.parent;
        }
        None
    }

    fn processes() -> HashMap<u32, Proc> {
        let mut out = HashMap::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE { return out; }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let exe = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
                out.insert(e.th32ProcessID, Proc { parent: e.th32ParentProcessID, exe });
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        out
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    unsafe fn read<T: Copy>(h: HANDLE, addr: usize) -> Option<T> {
        let mut v = std::mem::MaybeUninit::<T>::uninit();
        let mut n = 0usize;
        let ok = ReadProcessMemory(h, addr as *const c_void, v.as_mut_ptr().cast(), std::mem::size_of::<T>(), &mut n);
        (ok != 0 && n == std::mem::size_of::<T>()).then(|| v.assume_init())
    }

    /// Current directory from the PEB (64-bit processes only).
    fn process_cwd(pid: u32) -> Option<String> {
        // x64 offsets: PEB.ProcessParameters, RTL_USER_PROCESS_PARAMETERS.CurrentDirectory.DosPath.
        const PEB_PARAMS: usize = 0x20;
        const PARAMS_CWD: usize = 0x38;
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
            if h.is_null() { return None; }
            let h = Handle(h);
            let mut wow = 0;
            if IsWow64Process(h.0, &mut wow) == 0 || wow != 0 { return None; }
            // PROCESS_BASIC_INFORMATION; PebBaseAddress is the second field.
            let mut pbi = [0usize; 6];
            let size = std::mem::size_of_val(&pbi) as u32;
            if NtQueryInformationProcess(h.0, ProcessBasicInformation, pbi.as_mut_ptr().cast(), size, std::ptr::null_mut()) < 0 { return None; }
            let params: usize = read(h.0, pbi[1] + PEB_PARAMS)?;
            // UNICODE_STRING { Length: u16, MaximumLength: u16, Buffer: *u16 }
            let len: u16 = read(h.0, params + PARAMS_CWD)?;
            let buf: usize = read(h.0, params + PARAMS_CWD + 8)?;
            let mut chars = vec![0u16; len as usize / 2];
            let mut n = 0usize;
            if ReadProcessMemory(h.0, buf as *const c_void, chars.as_mut_ptr().cast(), len as usize, &mut n) == 0 { return None; }
            Some(String::from_utf16_lossy(&chars))
        }
    }

    fn top_windows() -> Vec<Win> {
        unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> i32 {
            let out = &mut *(lp as *mut Vec<Win>);
            if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() { return 1; }
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 { return 1; }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4) == 0 && cloaked != 0 { return 1; }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 { return 1; }
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            out.push(Win { hwnd, pid, class, title });
            1
        }
        let mut out: Vec<Win> = Vec::new();
        unsafe { EnumWindows(Some(cb), &mut out as *mut Vec<Win> as LPARAM) };
        out
    }

    unsafe fn activate(hwnd: HWND) {
        if IsIconic(hwnd) != 0 { ShowWindow(hwnd, SW_RESTORE); }
        // Borrow the foreground thread's input state so later windows can also be raised.
        let fg = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let me = GetCurrentThreadId();
        let attached = fg != 0 && fg != me && AttachThreadInput(me, fg, 1) != 0;
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if attached { AttachThreadInput(me, fg, 0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(path: &str) -> Target {
        let n = normalize(path, None).unwrap();
        Target { key: n.key, name: n.name, distro: None, path: path.into() }
    }

    fn wsl(distro: &str, path: &str) -> Target {
        let n = normalize(path, Some(distro)).unwrap();
        Target { key: n.key, name: n.name, distro: Some(distro.into()), path: path.into() }
    }

    #[test]
    fn title_with_path_marker() {
        let t = win(r"D:\work\repo");
        assert!(editor_title_matches(r"● a.rs - repo ⟨D:\work\repo⟩ - Visual Studio Code", &t));
        assert!(editor_title_matches(r"a.rs - sub ⟨d:\work\repo\sub⟩ - Cursor", &t));
        assert!(!editor_title_matches(r"a.rs - repo ⟨D:\other\repo⟩ - Visual Studio Code", &t));
        assert!(!editor_title_matches(r"a.rs - repo2 ⟨D:\work\repo2⟩ - Visual Studio Code", &t));
    }

    #[test]
    fn title_by_folder_name() {
        let t = win(r"D:\work\repo");
        assert!(editor_title_matches("Preview x.md - Preview x.md - Repo - Visual Studio Code", &t));
        assert!(editor_title_matches("[Administrator] ● repo - Antigravity IDE", &t));
        assert!(editor_title_matches("repo (Workspace) - Visual Studio Code - Insiders", &t));
        assert!(!editor_title_matches("repo [WSL: Ubuntu] - Visual Studio Code", &t));
        assert!(!editor_title_matches("repo - Google Chrome", &t));
        assert!(!editor_title_matches("Antigravity", &t));
    }

    #[test]
    fn title_for_wsl_project() {
        let t = wsl("Ubuntu", "/home/u/repo");
        assert!(editor_title_matches("a.rs - repo [WSL: Ubuntu] - Visual Studio Code", &t));
        assert!(editor_title_matches("a.rs - repo [WSL: Ubuntu] ⟨/home/u/repo⟩ - Visual Studio Code", &t));
        assert!(!editor_title_matches("a.rs - repo [WSL: Debian] - Visual Studio Code", &t));
        assert!(!editor_title_matches("a.rs - repo - Visual Studio Code", &t));
    }

    #[test]
    fn parent_folder_editor() {
        let t = win(r"D:\work\repo");
        assert_eq!(editor_ancestor_depth(r"a.rs - work ⟨D:\work⟩ - Visual Studio Code", &t), Some(r"d:\work".len()));
        assert_eq!(editor_ancestor_depth(r"a.rs - D:\ ⟨D:\⟩ - Cursor", &t), Some(3));
        assert_eq!(editor_ancestor_depth(r"a.rs - repo ⟨D:\work\repo⟩ - Visual Studio Code", &t), None);
        assert_eq!(editor_ancestor_depth(r"a.rs - wo ⟨D:\wo⟩ - Visual Studio Code", &t), None);
        assert_eq!(editor_ancestor_depth("a.rs - work - Visual Studio Code", &t), None);
        assert_eq!(editor_ancestor_depth(r"a.rs - work [WSL: Ubuntu] ⟨D:\work⟩ - Visual Studio Code", &t), None);
        let w = wsl("Ubuntu", "/home/u/repo");
        assert_eq!(editor_ancestor_depth("a.rs - u [WSL: Ubuntu] ⟨/home/u⟩ - Visual Studio Code", &w), Some("wsl:Ubuntu:/home/u".len()));
        assert_eq!(editor_ancestor_depth("a.rs - u [WSL: Ubuntu] ⟨~⟩ - Visual Studio Code", &w), None);
    }

    #[test]
    fn abbreviated_wsl_path_falls_back_to_name() {
        let w = wsl("Ubuntu", "/home/u/repo");
        assert!(editor_title_matches("a.rs - repo [WSL: Ubuntu] ⟨~/repo⟩ - Visual Studio Code", &w));
    }

    #[test]
    fn detects_title_without_marker() {
        assert_eq!(editor_without_marker("a.rs - repo - Visual Studio Code - Insiders"), Some("Code - Insiders"));
        assert_eq!(editor_without_marker("repo - Antigravity IDE"), Some("Antigravity IDE"));
        assert_eq!(editor_without_marker(r"a.rs - repo ⟨D:\repo⟩ - Cursor"), None);
        assert_eq!(editor_without_marker("Welcome ⟨⟩ - Visual Studio Code"), None);
        assert_eq!(editor_without_marker("Antigravity"), None);
    }

    #[test]
    fn cwd_inside_project() {
        let t = win(r"D:\work\repo");
        assert!(agent_cwd_matches(r"d:\work\repo\", &t));
        assert!(agent_cwd_matches(r"D:\work\repo\src", &t));
        assert!(!agent_cwd_matches(r"D:\work\repo2", &t));
    }
}
