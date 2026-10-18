# Agent Session Lens

[English](README.md) | 日本語

Claude Code と Codex のローカルセッションログ（読み取り専用）を監視し、プロジェクト単位で「今どのエージェントが何をしているか」を表示するデスクトップアプリ（Tauri + React + Rust）。WSL 上のセッションは `lens-wsl-helper` 経由で取り込みます。

## ビルドと実行

前提: Rust（stable）、Node.js、Tauri の前提環境（Windows では WebView2）。

```bash
npm install
npm run tauri dev     # 開発起動
npm run build         # 型検査 + フロントエンドビルド
cargo test --workspace
```

## 設定（settings.json）

- 場所: `%APPDATA%\agent-session-lens\settings.json`（`APPDATA` が無い場合は `$HOME/.config/agent-session-lens/settings.json`）。
- 形式: `{"version":1, "helperPath"?, "disabledDistros"?, "sourceRoots"?}`。壊れている場合は既定値で起動し、元ファイルを `settings.json.bad` に退避します。
- 通常は UI から変更します。手動編集は反映にアプリ再起動が必要な場合があります。

## WSL ヘルパー

WSL ディストロ内のログは Linux 側の `lens-wsl-helper`（既定 `/usr/local/bin/lens-wsl-helper`、`settings.json` の `helperPath` で変更可）が正規化 NDJSON として送ります。`cargo build -p lens-wsl-helper --release` で作成し、各ディストロへ配置してください。ホスト側は `wsl.exe` 経由で起動・監視します。

## ドキュメント

- 設計: [docs/architecture-v5.md](docs/architecture-v5.md)
- 実装（公開面・終了コード・設定・既知の注意点）: [docs/implementation-v5.md](docs/implementation-v5.md)

## ライセンス

[MIT License](LICENSE)
