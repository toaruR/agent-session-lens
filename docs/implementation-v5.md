# Implementation v5 (as implemented)

設計は `docs/architecture-v5.md`。本書は実装済みの公開面を実コードに合わせて記す（挙動の根拠: `crates/lens-core/src/ipc.rs`, `settings.rs`, `supervisor.rs`, `src-tauri/src/{main,commands,state,ingest,wsl}.rs`, `crates/lens-wsl-helper/src/{cli,main,serve}.rs`）。タスク別の検証結果は `docs/plans/implementation-status-v5.md`。

## 1. Tauri コマンド（6 本）

`tauri::generate_handler!` はこの 6 本のみ（`get_snapshot` は撤去済み）。コマンド本体は `src-tauri/src/commands.rs`、`generate_handler!` は `src-tauri/src/main.rs`。既存5コマンドのエラー型は `IpcError { code, message }`、`code` は snake_case（`not_ready` / `invalid_argument` / `project_not_found` / `distro_not_found` / `source_not_found` / `path_invalid` / `settings_save_failed`）。`message` は固定文言で入力値を含まない。`get_session_tokens` は固定の日本語エラー文字列を返す。

| コマンド | 引数 | 成功時 | エラー |
|---|---|---|---|
| `get_session_tokens` | `session_uid: string` | `TokenReport`（単体・子孫込み合計・子孫件数・不明/失敗件数。使用量記録なしは session が null） | セッション不明・親ログ読取不可・親の計算/保存失敗の固定文字列。子孫のみの失敗は判明分合計と failedCount で返す |
| `list_projects` | なし | `ProjectActivity[]`（lastUpdatedAt 降順） | `not_ready`（初回スキャン中・未開始） |
| `get_project_detail` | `key: string` | `Detail`（セッション本文は `sanitize_detail` で伏字化・切り詰め済み） | `invalid_argument`（key 空）、`project_not_found` |
| `get_health` | なし | `SourceHealth[]` | `not_ready`（Aggregator 未開始のみ。初回スキャン中は部分結果を返す） |
| `set_distro_enabled` | `distro: string`, `enabled: bool` | `{ "ok": true }` | `invalid_argument`（distro 空）、`distro_not_found`（既知 distro 外）、`settings_save_failed` |
| `set_source_root` | `source_id: string`, `path: string \| null` | `{ "ok": true }` | `invalid_argument`（空）、`source_not_found`、`path_invalid`、`settings_save_failed` |

`set_source_root` の `path_invalid`: Windows ソースはドライブ絶対パス（`C:\` / `C:/`）のみ、WSL ソース（`source_id` が `wsl:<distro>:<agent>`）は `/` 始まりのみ。UNC は不可。Windows パスは `read_dir` で読めること（`RealPathProbe`）。WSL パスはホストから検査できないためここでは通し、読めない場合はヘルパーが `root_unreadable` を返す。`path = null` は既定へ戻す（エントリ削除）。保存失敗時はメモリ上の設定も変えない。

引数名は Tauri 規約により JS 側では camelCase（`sourceId`）。

## 2. イベント（3 種）

`Ev` は untagged enum（`model::v5::Ev`）。`emit_ev`（`src-tauri/src/ingest.rs`）が下記名で `app.emit` する。

| イベント | ペイロード | 発火 |
|---|---|---|
| `projects_changed` | `{ upserts: ProjectActivity[], removes: string[], order: string[] }` | Aggregator の `take_events()` |
| `project_detail_changed` | `{ key: string }` | `projects_changed` の各 upsert について 1 件ずつ付随発火（`ev_project_detail_changed`） |
| `health_changed` | `{ sources: SourceHealth[] }` | 取り込みループ（TICK 毎）でヘルス配列が前回と変わったとき |

capabilities は `core:default`, `core:event:allow-listen`, `core:event:allow-unlisten` のみ。UI は `not_ready` を再試行しつつ `list_projects` / `get_health` を呼び、3 イベントを listen する。

## 3. settings.json

- 場所: `%APPDATA%\agent-session-lens\settings.json`（`APPDATA` 無し時は `$HOME/.config/agent-session-lens/`）。`settings_dir()` は `src-tauri/src/state.rs`。
- 形式: `{"version":1, "helperPath"?: "/abs", "disabledDistros"?: [string], "sourceRoots"?: {sourceId: path}}`。空の項目は書かない。`version` は 1 のみ。
- 保存: 同ディレクトリの `settings.json.tmp` に書き込み → `rename`。失敗時は `settings_save_failed`、メモリ上の値は不変。
- 読み込み（`SettingsStore::load` は失敗しない）:
  - ファイル無し: 既定値。初回保存時に作成。
  - JSON 不正・`version≠1`・型違反・`disabledDistros` 重複/空要素・`sourceRoots` 値が空/非文字列・UTF-8 不正・読み取りエラー: 既定値で起動し、元ファイルを `settings.json.bad` へ rename（rename 失敗は無視）、診断 `settings_load_failed`（count 1）。
  - `helperPath` が `/` 始まりでない: その項目のみ無視して `HELPER_PATH`（`/usr/local/bin/lens-wsl-helper`）を使用、`settings_load_failed`。
- 反映時点: `disabledDistros` は次の DISTRO_POLL_SECS、`sourceRoots` / `helperPath` は次のヘルパー起動（§6 caveat 参照）。

## 4. lens-wsl-helper CLI

`lens-wsl-helper --version` / `lens-wsl-helper serve --protocol <int> [--roots <abs>...]`。

| 終了 | stderr | 条件 |
|---|---|---|
| 0 | - | 正常。`--version` は `lens-wsl-helper <crate ver> protocol <PROTOCOL_VERSION>` を 1 行出力 |
| 2 | `usage_error` | 未知引数、サブコマンド無し、`--protocol` 欠落/非整数、`--roots` が相対パス、`--version` に余分な引数 |
| 3 | `protocol_unsupported` | `--protocol` ≠ `PROTOCOL_VERSION`（現在 1） |
| 4 | `root_unreadable` | `--roots` 指定があり全 root が読めない（一部のみ不可なら `error` を NDJSON で送って継続） |
| 5 | `stdout_failed` | stdout 書き込み/flush 失敗 |
| 101 | `internal` | panic（panic hook が stderr を `internal` のみにする）。`fault_inject` feature + `LENS_FAULT=panic` で試験可能 |

`main.rs` は `serve` の場合、先に `cli::run`（出力は捨てる）で引数検証・終了コード規約を通し、OK なら `serve::run_stdio` が実ストリームを開始する。`serve` は正規化済み NDJSON（hello, セッション, heartbeat, error）を stdout へ流す。HOME と `WSL_DISTRO_NAME` は Linux 側で解決。

## 5. モジュール表: crates/lens-core/src

| ファイル | 役割 |
|---|---|
| `lib.rs` | モジュール公開、`Engine` / `Snapshot` 再エクスポート |
| `consts.rs` | §2 既定値表（各定数はここだけで定義） |
| `clock.rs`, `clock_offset.rs` | 注入可能な時計、clockOffset 補正（中央値） |
| `change_detector.rs`, `runtime.rs` | デバウンス、ホットファイルのポーリング、定期 reconcile、バックオフ |
| `reader.rs`, `ndjson.rs` | 読み取り専用 JSONL tail reader（UTF-8 厳格、MAX_LINE_BYTES）、ヘルパー NDJSON 受信 |
| `claude.rs`, `codex.rs` | エージェント別パーサ |
| `session.rs` | ターン状態機械と status 判定表 |
| `engine/` | v3 由来のローカルソース探索・取り込み・`Snapshot` 集約。`mod.rs`（`Engine`・公開面）、`sources.rs`（`SourceConfig` / `default_sources`）、`parser.rs`（ファイル別パーサ）、`scan.rs`（rescan）、`ingest.rs`（差分読み取り）、`thread_names.rs`（Codex `session_index.jsonl` キャッシュ）、`snapshot.rs`（レガシー `Snapshot`） |
| `aggregator.rs` | v5 の 1 セッション表、プロジェクト集約、イベント生成（automated は非メンバー） |
| `project.rs` | Project Resolver（キー、表示名、git root、WSL パス変換） |
| `distro.rs` | WSL distro 検出（レジストリ）と稼働状態ポーリング |
| `supervisor.rs` | ヘルパー起動、heartbeat 監視、バックオフ再起動、終了コード/版ポリシー、取り込み |
| `process.rs` | 子プロセス spawn の唯一の入口 |
| `settings.rs` | settings.json の読み書き |
| `ipc.rs` | IPC 読み書き関数・Ev 生成・出力サニタイズ（Tauri 非依存） |
| `redact.rs` | 秘密伏字化と切り詰め |
| `model.rs` | 型定義（v3 `Snapshot` と `model::v5` の IPC 型） |
| `util.rs` | 内部ユーティリティ（非公開） |

他: `crates/lens-core/schema/lens.schema.json`（型定義、`tests/schema.rs` が設計書と一致を検証）、`tests/traceability.{rs,json}`（F-id 54 件 → テスト関数の対応を検証）、`crates/lens-wsl-helper/src/{main,cli,serve}.rs`、`src-tauri/src/{main,commands,state,ingest,wsl}.rs`、`src/{App.tsx,types.ts,format.ts}`。

## 6. 既知の注意点（caveats）

- レガシー `Engine::snapshot().sessions`（`engine/snapshot.rs`。互換のため残置、Tauri アプリは使わず Aggregator 経由）は Codex の automated セッションも列挙する。Aggregator はそれらをプロジェクトのメンバーから除外する（UI は Aggregator 経由）。
- WSL のレジストリ読み取りと実 `wsl.exe` 経路は CI 未検証。単体テストは `Launcher` / `Clock` / `SettingsFs` 等の注入で偽装している。
- ウィンドウ実行（`cargo tauri dev` 等の GUI 実機確認）は手動のみ。自動試験対象外。
- WSL セッションの status は Aggregator が `wsl_input()`（`src-tauri/src/wsl.rs`）で `UnifiedSession` から `SessionInput` へ戻して再評価する（ヘルパー側の status をそのまま使わない）。
- ローカル（Windows）の `sourceRoots` は設定に保存されるが、起動済みの `Engine` には適用されない。反映は次回起動時。WSL の `sourceRoots` は次のヘルパー起動で `--roots` に反映。
- `cargo clippy --workspace --all-targets -- -D warnings` は `-A` 無しで通る（`model::Msg` の `large_enum_variant` のみ理由付き `#[allow]`。公開 API を変えないため）。検証に `npx tsc --noEmit` も含める。v3 由来の `Engine` / `model::Snapshot` は残置。

## 7. 検証スクリプトと追加テスト

- `scripts/verify-final-state.py`（stdlib のみ、`--write-md` で `docs/plans/implementation-status-v5.md` の生成セクションを更新）: 固定プランの verify 181 コマンドを Git Bash で実行し、「期待終了コード一致」または「後続タスクによる置換の機械的証明付き」でなければ失敗する。あわせて fileset からプラン `changes[].path` の和集合を引いた計画外パスに根拠が無ければ失敗する。
- 追加テスト: `src/format.test.ts`（`node --test src/format.test.ts`、relativeTime / updatingLabel の境界。tsconfig の `exclude` で tsc 対象外）、`src-tauri/src/wsl.rs` の `#[cfg(test)]`（`parse_ts`、`wsl_input` の status→turn 変換、degraded 判定。計 4 本）。

## 8. コンパクト表示

- 画面内のアプリ名は表示せず、OS のウィンドウタイトルにのみ表示する。
- 表示幅が 900 CSS px 未満ではプロジェクト一覧だけを表示し、プロジェクト名と更新時刻の2列にする。列見出し、WSL・停止中の補足、エージェント列、セッション詳細、ログ収集の状態は隠す。
- コンパクト表示の外側の余白・枠線はなく、一覧内の余白は 4px。長いプロジェクト名は折り返す。900px 以上では通常表示へ戻り、選択・開閉状態を維持する。
- ネイティブウィンドウの最小サイズは 320 × 200。表示幅の判定は WebView の CSS px に従う。
- `scripts/verify-session-tokens.mjs` は Tauri IPC を偽装したブラウザーで 320 / 600 / 899px と 900 / 1280px の表示・状態復元・横にはみ出さないことを検証する。OS の実ウィンドウ操作は対象外。
