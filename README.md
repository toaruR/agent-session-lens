# Agent Session Lens

English | [日本語](README.ja.md)

> [!WARNING]
> This project is currently under active development (WIP).

A desktop application (Tauri + React + Rust) that monitors local session logs (read-only) of Claude Code and Codex, displaying what each agent is currently doing on a per-project basis in real time. Sessions running inside WSL are ingested via `lens-wsl-helper`.

## Screenshots

### Normal mode

![Agent Session Lens — normal mode](docs/images/screenshot.png)

### Compact mode

Below 900 px wide, the app shows a compact project list.

![Agent Session Lens — compact mode](docs/images/screenshot-compact.png)

Screenshots use sample data.

## Project list actions

- Click: show session details.
- Shift+click: open the project folder in Explorer.
- Ctrl+click: bring windows working on the project to the front.
  - Editors (VS Code / Cursor / Antigravity IDE) are matched by the folder name in the window title, or by the full path when the title contains `⟨${rootPath}⟩` (the path marker of [vscode-selector](https://github.com/toaruR/vscode-selector)). If an open editor lacks the marker, the app offers to add it to `window.title` in that editor's settings.json (the original is kept as `.bak`; declining is remembered until the app exits).
  - Agent CLIs (`claude` / `codex` / `agy`) are matched by working directory, and the terminal or desktop app window running them is raised. Agents inside an editor are covered by the editor. Processes inside WSL are not matched.

## Build and Run

Prerequisites: Rust (stable), Node.js, and Tauri prerequisites (e.g., WebView2 on Windows).

```bash
npm install
npm run tauri dev     # Development mode
npm run build         # Type check + frontend build
npm run tauri -- build --no-bundle # Release executable with embedded frontend
cargo test --workspace
```

## Configuration (settings.json)

- Location: `%APPDATA%\agent-session-lens\settings.json` (falls back to `$HOME/.config/agent-session-lens/settings.json` if `APPDATA` is unset).
- Format: `{"version":1, "helperPath"?, "disabledDistros"?, "sourceRoots"?}`. If corrupted, the application starts with default values and backs up the invalid file to `settings.json.bad`.
- Typically modified via the UI. Manual modifications may require restarting the application to take effect.

## WSL Helper

Logs inside WSL distributions are streamed as normalized NDJSON by the Linux-side `lens-wsl-helper` (default `/usr/local/bin/lens-wsl-helper`, configurable via `helperPath` in `settings.json`). The host launches and monitors it via `wsl.exe`.

Install in each distribution to monitor, from PowerShell at the repository root:

```powershell
./scripts/install-wsl-helper.ps1 -Distro Ubuntu
```

This builds with Rust **inside the selected WSL distribution**, installs the Linux executable with WSL root into `/usr/local/bin`, and checks `--version`. Rust and a C compiler must already be installed there; Cargo may download dependencies. A Windows `cargo build` cannot supply this Linux helper. Installation starts the selected distribution; monitoring leaves stopped distributions asleep. `helper_not_found` means the configured executable is missing. The running app retries after installation; a host code update requires restarting the rebuilt app.

Verify the complete Windows-to-WSL ingest path against existing logs:

```powershell
$env:LENS_TEST_DISTRO = 'Ubuntu'
cargo test -p lens-core --test real_helper_process real_wsl_reads_existing_sources -- --ignored --nocapture
```

This reports source health even when all logs are outside the 7-day initial scan window (`skippedOldFiles`). To additionally require sessions, run the ignored test `real_wsl_ingests_existing_sessions` against recent logs; `LENS_TEST_CLAUDE_ROOT` and `LENS_TEST_CODEX_ROOT` can select temporary Linux fixture directories without changing settings or agent logs.

## Documentation

- Architecture: [docs/architecture-v5.md](docs/architecture-v5.md)
- Implementation (Public surface, exit codes, settings, and known notes): [docs/implementation-v5.md](docs/implementation-v5.md)

## License

[MIT License](LICENSE)
