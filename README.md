# Agent Session Lens

English | [日本語](README.ja.md)

A desktop application (Tauri + React + Rust) that monitors local session logs (read-only) of Claude Code and Codex, displaying what each agent is currently doing on a per-project basis in real time. Sessions running inside WSL are ingested via `lens-wsl-helper`.

## Build and Run

Prerequisites: Rust (stable), Node.js, and Tauri prerequisites (e.g., WebView2 on Windows).

```bash
npm install
npm run tauri dev     # Development mode
npm run build         # Type check + frontend build
cargo test --workspace
```

## Configuration (settings.json)

- Location: `%APPDATA%\agent-session-lens\settings.json` (falls back to `$HOME/.config/agent-session-lens/settings.json` if `APPDATA` is unset).
- Format: `{"version":1, "helperPath"?, "disabledDistros"?, "sourceRoots"?}`. If corrupted, the application starts with default values and backs up the invalid file to `settings.json.bad`.
- Typically modified via the UI. Manual modifications may require restarting the application to take effect.

## WSL Helper

Logs inside WSL distributions are streamed as normalized NDJSON by the Linux-side `lens-wsl-helper` (default `/usr/local/bin/lens-wsl-helper`, configurable via `helperPath` in `settings.json`). Build with `cargo build -p lens-wsl-helper --release` and place it in each distribution. The host launches and monitors it via `wsl.exe`.

## Documentation

- Architecture: [docs/architecture-v5.md](docs/architecture-v5.md)
- Implementation (Public surface, exit codes, settings, and known notes): [docs/implementation-v5.md](docs/implementation-v5.md)

## License

[MIT License](LICENSE)
