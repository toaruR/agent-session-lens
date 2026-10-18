# Agent Session Lens アーキテクチャ設計書 v5

Claude Code / Codex CLI のローカルセッションログ（Windows と WSL2）を読み取り専用で監視し、プロジェクトを最終更新順に並べる Tauri v2 (Rust + React) ビューア。更新の判断はセッション情報だけで行い、作業ツリーの mtime と Git は見ない。単体で完結する文書。

## 1. 原則と構成

1. 元ログに書き込まない（共有読み取りのみ）。読めなければ `unavailable`。
2. 帰属と状態はログ内メタデータ（`cwd`、レコード種別）だけで決め、本文中のパスから推測しない。
3. 欠落を隠さない。読めない・未知のレコードは診断コード付きで `partial` / `unknown`。
4. 形式は版でなく実レコードで判定する。本文・オフセット・集計値・診断はメモリのみ（再起動で初回走査からやり直す）。ディスクに置くのは §6.1 の `settings.json`（利用者設定だけ）。
5. WSL のログは WSL 内ヘルパーが読み、正規化済み結果を NDJSON で送る。`\wsl.localhost`（9P）は変更通知が届かないため読まない。

## 2. 既定値表（定数はここで 1 回だけ定義し、他の節は名前で参照する）

| 名前 | 値 | 理由 |
|---|---|---|
| STALE_SECS | 600 | 10 分無更新なら作業中と見なさない |
| RECENT_WRITE_SECS | 30 | ツール未解決でも直近追記があれば high とする窓 |
| CLOCK_SKEW_MS | 5000 | 揺らぎと復帰後のずれを分ける閾値 |
| REEVAL_SECS | 5 | `updating` / `stale` の再評価周期。切替の観測遅れは最大 REEVAL_SECS−1 秒。独立根拠: 相対時刻の単位（1 分）より十分短く、再評価はメモリ上の比較だけ |
| MAX_LINE_BYTES | 4194304 | 改行を含まない 1 行の上限 |
| HEAD_SCAN_BYTES / TAIL_SCAN_BYTES | 524288 / 8388608 | 初回の先頭（title）と末尾（最新状態）。中間は `truncated_initial_scan` |
| DEBOUNCE_MS | 200 | notify の連続追記を 1 回にまとめる最短値 |
| HOT_WINDOW_SECS / HOT_POLL_SECS | 900 / 2 | 直近 15 分更新のファイルを 2 秒ごとに (size, mtime) 確認。通知欠落の検出遅延上限。独立根拠: 追記の遅延を 2 秒に抑え、stat のみ・直近 15 分分に限るので負荷が小さい |
| RECONCILE_SECS | 60 | ディレクトリ照合周期。独立根拠: 新規ファイルの検出遅延を 1 分以内にしつつ、7 日分の列挙を 1 分 1 回に抑える |
| INITIAL_SCAN_DAYS | 7 | 初回走査は `mtime ≥ now_ms − 7 日`（ちょうど 7 日は読む）。以前は件数のみ記録 |
| DISTRO_POLL_SECS | 60 | `wsl.exe --list --running --quiet` の周期 |
| HB_INTERVAL_SECS / HB_TIMEOUT_SECS / HB_MEDIAN_N | 10 / 30 / 5 | heartbeat 周期 / 無応答で再起動する経過秒（周期の 3 倍）/ clockOffset 中央値の個数 |
| BACKOFF_MAX_SECS | 60 | 再起動・再試行の待ち `min(2^(n−1), 60)` 秒（n は連続失敗回数、成功で n=1） |
| PROTOCOL_VERSION | 1 | NDJSON 版。不一致は即停止 |
| RESOLVE_MAX_DEPTH | 32 | `.git` 上方探索の最大階層 |
| TRUNC_TEXT_CHARS / TRUNC_OUT_CHARS | 2000 / 1000 | 発言・ツール入力の上限 / ツール出力は先頭と末尾それぞれ |
| SK_MIN / GH_LEN / AWS_LEN | 20 / 36 / 16 | R1: 20 未満の `sk-` 語は鍵でなく誤検出。R2: GitHub トークン本体は固定 36 文字。R3: AWS キー ID は `AKIA` + 16 の固定 20 文字 |
| R4_WORDS | TOKEN / SECRET / PASSWORD / API_?KEY / CSRF | R4 の語彙。CSRF は実ログの `ANTIGRAVITY_CSRF_TOKEN=…`、他は一般的な変数名（`GITHUB_TOKEN`、`CLIENT_SECRET`、`DB_PASSWORD`、`OPENAI_API_KEY`） |
| SETTINGS_PATH | `%APPDATA%\agent-session-lens\settings.json` | 利用者設定の唯一の保存先（§6.1） |
| HELPER_PATH | `/usr/local/bin/lens-wsl-helper` | `--exec` は `~` を展開しないため絶対パス。上書きは settings.json の `helperPath` のみ（環境変数・UI 入力は持たず保存先を 1 つにする） |
| now_ms | Windows 現在時刻（ms） | 時刻比較の基準。WSL 時計は使わない |
| age_s | `floor((now_ms − t_corrected_ms)/1000)` | t は §3.2 補正後。整数秒で比較 |

既定パス: `%USERPROFILE%\.claude\projects`（`CLAUDE_CONFIG_DIR`）、`%USERPROFILE%\.codex\sessions`（`CODEX_HOME`）。WSL は `$HOME` 配下の同名（上書きは `--roots`）。利用者定義 redaction は持たない。

## 3. 判定規則と検算

### 3.1 stale と status（適用順は # の昇順。最初に当たった行を採るので 1 が 5 に必ず先行する）

open ターン = 開始があり完了・中断が無い。tool_open = 未解決のツール呼び出しがある。fresh = `age_s < STALE_SECS`、stale = `age_s ≥ STALE_SECS`。末尾欠落 = 初回走査の TAIL_SCAN_BYTES 内に完全な行が 0 個。

| # | 条件 | value | confidence | stale |
|---|---|---|---|---|
| 1 | 最新レコードを読めない（未知形式、最終行が `invalid_utf8` / `oversized_line`、末尾欠落） | unknown | low | false |
| 2 | 最後のターンがエラー終了、以降の活動なし | fresh: error / stale: idle | high | false |
| 3 | open ターン | running | stale: low。fresh かつ（tool_open または `age_s < RECENT_WRITE_SECS`）: high。他: low | `age_s ≥ STALE_SECS` |
| 4 | 最後のターンが完了・中断 | fresh: waiting_input / stale: idle | high | false |
| 5 | 1〜4 以外（完全な行はあるがターン境界が見えない） | fresh: running / stale: idle | low | false |

`updating` = 帰属セッションに `running ∧ stale=false` が 1 つ以上。low のみなら `updatingConfidence=low`（UI は「更新中?」）。automated は含めない。

検算（lastActivityAt=T、now_ms=T+age_s）:

| 入力 | age_s | tool_open | 出力 (value, confidence, stale, updating) |
|---|---|---|---|
| open ターン | 29 / 30 | なし | running, high / low, false, true |
| open ターン | 599 | あり | running, high, false, true |
| open ターン | 600 / 601 | あり | running, low, true, false |
| 完了 / エラー終了 | 599 → 600 | — | waiting_input → idle / error → idle |

相対時刻は `floor(age_s/60)分前`（180 → 「3分前」、逆に 180 ≤ age_s ≤ 239。60 未満は「1分未満」）。ヘルパー送信の status は Lens が now_ms で再評価する。

追加規則:
- Claude は `tool_use` 後に tool_result が来てもターン未完了。承認待ち・長時間コマンド・クラッシュは区別できず `awaiting_approval` は出さない（F14）。
- 親のターンが子（subagent・fork）の最後の活動より後に終わっていれば、子が running でも idle（low、reason `parent turn ended after last activity`）。guardian_review（automated）の子が running の間、親は `autoReview=true`（親の value は変えない）。
- ターン信号。Claude: 人の発言（開始）、`end_turn`（完了）、`[Request interrupted by user`（中断）、`system/api_error`（エラー）。Codex: `task_started`、`task_complete`（`error` ありでエラー）、`turn_aborted`。

### 3.2 clockOffset 補正

`offset_ms = recv_ms − helper_now_ms`（recv_ms は Windows 時計の受信時刻）。`hello` と `heartbeat` ごとのサンプルの直近 HB_MEDIAN_N 個の中央値を採る。`|offset_ms| ≤ CLOCK_SKEW_MS`（−5000, 0, 5000）は補正しない。`>`（−5001, 5001）は `t_corrected_ms = t_helper_ms + offset_ms` で補正し `clock_skew`。補正後に `t_corrected_ms > now_ms` なら `now_ms` に丸めて `clock_skew`（補正 → 丸めの順、許容幅なし）。

検算: helper `now`=12:00:00.000、受信 12:00:08.000 → offset_ms=8000 → 補正。`lastActivityAt`=11:59:50.000 は 11:59:58.000 になり、`− 8000 ms` で元に戻る。サンプル [7000, 8000, 8000, 9000, 40000] の中央値は 8000。

### 3.3 数値の往復検算

| 対象 | 入力 → 出力 | 復元する値 |
|---|---|---|
| stale 切替の最大遅延 | age_s=599 の評価直後に 600 になると次の評価は 604。初観測の age_s ∈ 600..604、605 に持ち越さない | 最大遅れ 604−600 = 4、最小 0（評価時に 600）。遅れ 0〜4 = REEVAL_SECS−1 なので REEVAL_SECS = 4+1 = 5 |
| MAX_LINE_BYTES の境界 | 内容 4194304 B + `\n`（ファイル上 4194305 B）は処理、内容 4194305 B + `\n` は破棄。改行無し末尾は内容 4194304 B まで保持、4194305 B で破棄 | 処理できる最大の内容 4194304 = MAX_LINE_BYTES（改行は数えない） |
| バックオフ | n=1..8 → 1, 2, 4, 8, 16, 32, 60, 60 秒（n=7 は 64→60）。成功後は n=1 → 1 秒 | 最大 60 = BACKOFF_MAX_SECS |

## 4. 「更新」の定義

- `lastActivityAt` = 活動イベント時刻（`timestamp`）の最大値（`event`）。活動が無いときだけファイル mtime（`file_mtime`）。
- `lastUpdatedAt`（プロジェクト）= 帰属セッションの `lastActivityAt` の最大値（subagent・fork を含む）。降順、同時刻は名前の昇順。帰属不明（`projects=[]`）は 1 行。
- 活動イベント。Claude: 人の発言、assistant、tool_result、`system/api_error`。Codex: `response_item`、`task_started` / `task_complete` / `turn_aborted`。数えない: Claude の `custom-title` / `ai-title` / `summary` / `attachment` / `isMeta` / `api-request-blob`、Codex の `session_meta` のみのファイル・fork 継承範囲・guardian_review・`session_index.jsonl`。

## 5. エージェント別仕様

**Claude Code**: `projects/<enc>/<sessionId>.jsonl` と `…/<sessionId>/subagents/agent-<agentId>.jsonl`。subagent は親と同じ `sessionId`、子 ID は `sessionId::agent:<agentId>`。別 sessionId が混在するファイルは `mixed_session_ids` で除外。project は各レコードの `cwd`。人の発言 = `type=user` ∧ `isMeta` 無し ∧ tool_result 無し。title は `custom-title`、`ai-title`、最初の人の発言の順。`api-request-blob` があるため先に `type` だけを probe し必要な行だけ parse。`type` を取れない行は MAX_LINE_BYTES 以下のときだけ完全 parse。

**Codex CLI**: `sessions/YYYY/MM/DD/rollout-*.jsonl`。ID と project は先頭の `session_meta.payload.{id,cwd}`（2 つ目以降は fork 元で上書きしない）。`thread_source`: `user` → top_level、`subagent` → subagent、`guardian_review` → automated（一覧に出さず親の `autoReview` に集約）、未知は top_level + `unknown_thread_source`。本文は `response_item` の `message` を正とし、`event_msg` は状態判定にだけ使う。title は `thread_name`（`session_index.jsonl`）、無ければ最初の人の発言。
fork 継承レコードの `timestamp` は fork 時刻に書き換わる。0 始まりの行番号が `session_meta.subagent_history_start_ordinal` 未満は活動に数えない。ordinal が無く `session_meta` が 2 つなら境界不明のため全体を取り込み `fork_boundary_unknown` + low。

**Antigravity (AGY)**: `%USERPROFILE%\.gemini\{antigravity,antigravity-cli,antigravity-ide}\conversations\<uuid>.db`（上書きは `ANTIGRAVITY_HOME`）。SQLite を `mode=ro` かつ `PRAGMA query_only=ON` で開き、`-wal` 更新を検知して再読込する（WAL 未反映を防ぐため `immutable=1` は使用しない）。ID は `trajectory_meta.cascade_id`（またはファイル幹 UUID）。project は `trajectory_metadata_blob` の Field 7（または 1.1）の URI（`outside-of-project` 等の欠落時は None）。人の発言 = `step_type=14` の payload Field 19.2。title は最初の人の発言。アシスタント応答 = `step_type=15` の payload Field 20.1/8。ツール呼出 = `step_type=15` の Field 20.7（1=call_id, 2=name, 3=input）、旧コマンド = `step_type=21` の Field 28.23/25、ツール結果 = `step_type=132`。各ステップの `metadata` Field 1（`google.protobuf.Timestamp`）から時刻を取得。

## 6. WSL ヘルパー

- ディストロ一覧は `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss\{GUID}` の `DistributionName` / `Version`（WSL を起こさない）。`Version=2` だけ対象、他は `unsupported`。稼働確認は DISTRO_POLL_SECS ごと。停止中は起動せず、`disabledDistros` は監視しない。
- 起動: `wsl.exe -d <distro> --exec <helperPath または HELPER_PATH> serve --protocol 1 [--roots <path>…]`（既定ユーザーのホームのみ）。stdin は `Stdio::null()`、stdout は NDJSON、stderr は診断コードのみ。無ければ `helper_unavailable`（detail `helper_not_found`）。
- 監視: HB_TIMEOUT_SECS 無応答かプロセス終了で、稼働中なら §3.3 のバックオフで再起動、停止中なら「停止中」（最後の状態を `originAvailable=false`、`updating=false` で表示）。子は Job Object に入れ、再起動時は状態を作り直し初回走査からやり直す。ヘルパーは切り詰めと redaction 済みで送り、Lens も再度 redaction をかける。
- 帰属: cwd が `/mnt/<drive>/…` なら Windows パスと同じキーで Windows の行に統合。それ以外は `wsl:<distro>:/home/u/p`（大文字小文字を保つ）、表示 `p (WSL: <distro>)`。Windows 側 cwd が `\wsl.localhost\<distro>\…` / `\wsl$\<distro>\…` なら同じ `wsl:` キーに文字列で変換。

### 6.1 設定（settings.json）

保存先は SETTINGS_PATH の 1 か所。`set_distro_enabled` は `disabledDistros`、`set_source_root` は `sourceRoots`、HELPER_PATH の上書きは `helperPath` に書く（スキーマは §7 の `Settings`）。同ディレクトリの一時ファイルへ書いて rename する。

| 状況 | 動作 |
|---|---|
| ファイル無し | 既定値で起動。作成は最初の保存時 |
| 読み込み失敗（JSON 不正・スキーマ不一致・`version≠1`） | 既定値で起動し、壊れたファイルを `settings.json.bad` に退避、診断 `settings_load_failed` |
| 保存失敗（権限・容量・rename） | メモリ上の設定を変えず、IPC は `settings_save_failed`。再試行は利用者の再操作 |
| `helperPath` が `/` で始まらない | その項目だけ無視して HELPER_PATH を使い、`settings_load_failed` |
| 反映時点 | `disabledDistros` は次の DISTRO_POLL_SECS、`sourceRoots` と `helperPath` は次のヘルパー起動 |

### 6.2 ヘルパー CLI

`lens-wsl-helper serve --protocol <int> [--roots <abs-path>…]` と `lens-wsl-helper --version`（版を 1 行出力、終了 0）。

| 条件 | 終了 | stderr | Lens 側 |
|---|---|---|---|
| 未知の引数・サブコマンド無し・`--protocol` 欠落か非整数・`--roots` が相対 | 2 | `usage_error` | `helper_unavailable`（detail `usage_error`）、再起動しない |
| `--protocol` ≠ PROTOCOL_VERSION | 3 | `protocol_unsupported` | `helper_version_mismatch`、再起動しない |
| 全 root が読めない | 4 | `root_unreadable` | バックオフで再起動 |
| stdout 書き込み失敗 | 5 | `stdout_failed` | バックオフで再起動 |
| panic | 101 | `internal` | バックオフで再起動 |

一部の root だけ読めないときは `error`（`root_unreadable`）を送って継続する。

### 6.3 診断コードと NDJSON 受信

診断コード。行: `malformed_record`、`invalid_utf8`、`oversized_line`（いずれも行を捨てる）、`incomplete_tail`（保留、情報のみ）。ファイル: `file_replaced`（先頭から再読込）、`truncated_initial_scan` / `mixed_session_ids` / `unknown_thread_source` / `unknown_record`（`partial`）、`fork_boundary_unknown`（low）、`clock_skew`。NDJSON: `invalid_message`、`unknown_message`、`helper_version_mismatch`、`root_unreadable`、`internal`。CLI: `usage_error`、`protocol_unsupported`、`stdout_failed`。設定: `settings_load_failed`、`settings_save_failed`。

NDJSON は helper → Lens の 1 行 1 メッセージ（各行 MAX_LINE_BYTES 以下、型は §7 の `Msg`、時刻は RFC 3339 UTC、未知フィールドは無視、応答チャネルなし）。`hello` が最初の 1 行、`session_upsert` は起動直後は全件・以降は差分、`session_remove` はログ消失時、`source_health` は `{sourceId, health: SourceHealth}`（hello 直後に全 source 分、以降は変化時）、`heartbeat` は HB_INTERVAL_SECS ごと、`error` は継続不能か一部 root 不可（本文・パスなし）。

| 受信条件 | 処理 | 診断 / 状態 |
|---|---|---|
| JSON でない / UTF-8 不正 / MAX_LINE_BYTES 超 | 行を捨てる | `malformed_record` / `invalid_utf8` / `oversized_line` |
| 必須欠落・型不一致・`hello` 前の `session_upsert` / `source_health` | 行を捨てる | `invalid_message` |
| 未知の `type` | 無視 | `unknown_message` |
| `hello.protocol ≠ PROTOCOL_VERSION` | 停止、再起動しない | `helper_version_mismatch` |

## 7. データモデルと JSON Schema（draft-07。IPC と NDJSON の唯一の型定義）

```json
{"$schema":"http://json-schema.org/draft-07/schema#","definitions":{
"ts":{"type":"string","format":"date-time"},
"N":{"type":"integer","minimum":0},
"Agent":{"enum":["claude_code","codex","agy"]},
"Src":{"enum":["event","file_mtime"]},
"Conf":{"enum":["high","low"]},
"Diag":{"type":"object","required":["code","count"],"properties":{"code":{"type":"string"},"count":{"$ref":"#/definitions/N"}}},
"Origin":{"type":"object","required":["kind"],"properties":{"kind":{"enum":["windows","wsl"]},"distro":{"type":"string"}}},
"ProjectActivity":{"type":"object","required":["key","name","origin","lastUpdatedSource","updating","originAvailable","agents","sessionUids"],"properties":{"key":{"type":"string","minLength":1},"name":{"type":"string"},"origin":{"$ref":"#/definitions/Origin"},"lastUpdatedAt":{"$ref":"#/definitions/ts"},"lastUpdatedSource":{"$ref":"#/definitions/Src"},"updating":{"type":"boolean"},"updatingConfidence":{"$ref":"#/definitions/Conf"},"originAvailable":{"type":"boolean"},"agents":{"type":"array","items":{"$ref":"#/definitions/Agent"}},"sessionUids":{"type":"array","items":{"type":"string"}}}},
"UnifiedSession":{"type":"object","required":["sessionUid","nativeId","agent","sourceId","origin","projects","lastActivitySource","status","currentTask","hierarchy","health"],"properties":{"sessionUid":{"type":"string"},"nativeId":{"type":"string"},"agent":{"$ref":"#/definitions/Agent"},"sourceId":{"type":"string"},"origin":{"$ref":"#/definitions/Origin"},"lastActivityAt":{"$ref":"#/definitions/ts"},"lastActivitySource":{"$ref":"#/definitions/Src"},
"projects":{"type":"array","items":{"type":"object","required":["key","name","displayPath","attribution"],"properties":{"key":{"type":"string","minLength":1},"name":{"type":"string"},"displayPath":{"type":"string"},"attribution":{"enum":["git_root","raw_path"]}}}},
"status":{"type":"object","required":["value","reason","confidence","stale","autoReview"],"properties":{"value":{"enum":["running","waiting_input","idle","error","unknown"]},"confidence":{"$ref":"#/definitions/Conf"},"stale":{"type":"boolean"},"autoReview":{"type":"boolean"}}},
"currentTask":{"type":"object","properties":{"title":{"type":"string"},"lastUserMessage":{"type":"string"},"lastAssistantMessage":{"type":"string"},"activeTool":{"type":"object","required":["name","input"]}}},
"hierarchy":{"type":"object","required":["childUids","kind"],"properties":{"kind":{"enum":["top_level","subagent","fork","automated"]}}},
"health":{"type":"object","required":["completeness","diagnostics"],"properties":{"completeness":{"enum":["complete","partial","unsupported","unavailable"]},"diagnostics":{"type":"array","items":{"$ref":"#/definitions/Diag"}}}}}},
"SourceHealth":{"type":"object","required":["sourceId","origin","rootPath","state","filesTracked","sessionsTotal","partialSessions","skippedOldFiles","topDiagnostics"],"properties":{"agent":{"$ref":"#/definitions/Agent"},"origin":{"$ref":"#/definitions/Origin"},"state":{"enum":["ok","not_installed","empty","access_denied","unsupported_schema","error","distro_stopped","helper_unavailable","helper_version_mismatch","unsupported"]},"lastScanAt":{"$ref":"#/definitions/ts"},"filesTracked":{"$ref":"#/definitions/N"},"sessionsTotal":{"$ref":"#/definitions/N"},"partialSessions":{"$ref":"#/definitions/N"},"skippedOldFiles":{"$ref":"#/definitions/N"},"topDiagnostics":{"type":"array","items":{"$ref":"#/definitions/Diag"}}}},
"Settings":{"type":"object","required":["version"],"properties":{"version":{"const":1},"helperPath":{"type":"string","pattern":"^/"},"disabledDistros":{"type":"array","uniqueItems":true,"items":{"type":"string","minLength":1}},"sourceRoots":{"type":"object","additionalProperties":{"type":"string","minLength":1}}}},
"IpcError":{"type":"object","required":["code","message"],"properties":{"code":{"enum":["not_ready","invalid_argument","project_not_found","distro_not_found","source_not_found","path_invalid","settings_save_failed"]},"message":{"type":"string"}}},
"In":{"type":"object","oneOf":[{"maxProperties":0},{"required":["key"],"properties":{"key":{"type":"string","minLength":1}}},{"required":["distro","enabled"],"properties":{"distro":{"type":"string","minLength":1},"enabled":{"type":"boolean"}}},{"required":["sourceId","path"],"properties":{"sourceId":{"type":"string","minLength":1},"path":{"type":["string","null"],"minLength":1}}}]},
"Detail":{"type":"object","required":["project","sessions"],"properties":{"project":{"$ref":"#/definitions/ProjectActivity"},"sessions":{"type":"array","items":{"$ref":"#/definitions/UnifiedSession"}}}},
"Ev":{"type":"object","oneOf":[{"required":["upserts","removes","order"],"properties":{"upserts":{"type":"array","items":{"$ref":"#/definitions/ProjectActivity"}}}},{"required":["key"]},{"required":["sources"],"properties":{"sources":{"type":"array","items":{"$ref":"#/definitions/SourceHealth"}}}}]},
"Msg":{"type":"object","required":["type"],"oneOf":[
{"required":["protocol","helperVersion","distro","home","now"],"properties":{"type":{"const":"hello"},"protocol":{"type":"integer"},"helperVersion":{"type":"string"},"distro":{"type":"string","minLength":1},"home":{"type":"string","pattern":"^/"},"now":{"$ref":"#/definitions/ts"}}},
{"required":["sourceId","health"],"properties":{"type":{"const":"source_health"},"health":{"$ref":"#/definitions/SourceHealth"}}},
{"required":["session"],"properties":{"type":{"const":"session_upsert"},"session":{"$ref":"#/definitions/UnifiedSession"}}},
{"required":["sessionUid"],"properties":{"type":{"const":"session_remove"}}},
{"required":["now"],"properties":{"type":{"const":"heartbeat"},"now":{"$ref":"#/definitions/ts"}}},
{"required":["code"],"properties":{"type":{"const":"error"},"code":{"enum":["root_unreadable","internal"]}}}]}}}
```

文字列フィールド（`sessionUid`、`key`、`name` など）は string。`sessionUid` = `${sourceId}:${nativeId}`。`sourceId` = `claude-local` / `codex-local` / `wsl:<distro>:claude|codex`。帰属不明の `key` は `"unattributed"`。`projects_changed` は `{upserts, removes, order}`、`project_detail_changed` は `{key}`、`health_changed` は `{sources}`（`Ev`）。automated は ProjectActivity に含めない。

## 8. IPC と実装

コマンド（Tauri `invoke`。入力は `In`、エラーは `IpcError`、message に本文・パスなし）:

| コマンド | 入力 | 出力 | エラー code |
|---|---|---|---|
| `list_projects` | `{}` | `ProjectActivity[]`（並び順済み） | `not_ready`（初回走査中） |
| `get_project_detail` | `{key}` | `Detail` | `invalid_argument`（key 空）, `project_not_found` |
| `get_health` | `{}` | `SourceHealth[]` | `not_ready`（Aggregator 起動前のみ。初回走査中は途中経過を返す。他の code は返さない） |
| `set_distro_enabled` | `{distro, enabled}` | `{ok: true}` | `invalid_argument`, `distro_not_found`, `settings_save_failed` |
| `set_source_root` | `{sourceId, path}`（null で既定に戻す） | `{ok: true}` | `source_not_found`, `path_invalid`（Windows はドライブ絶対パス、WSL は `/` 始まり。UNC `\\` と読めないパスは不可）, `settings_save_failed` |

イベントの payload は切り詰め・redaction 済みの `Ev`: `projects_changed`（`{upserts, removes, order}`。内容・並びが変化したときだけ。order は全 key の新しい順）、`project_detail_changed`（`{key}`。UI は `get_project_detail` を再取得）、`health_changed`（`{sources}`）。

- Reader: Windows は共有モード `READ | WRITE | DELETE` で開き読み終えたら閉じる。状態 `(fileId, offset, size, mtime)`。fileId 変更かサイズ縮小は先頭から再読込（`file_replaced`）。offset は完全な行の末尾でだけ進め、改行で終わらない末尾はバッファに残す（`incomplete_tail`）。UTF-8 は 1 行ごとに厳格デコードし失敗行は捨てる（置換も CP932 推定もしない）。
- Change Detector: `notify` を DEBOUNCE_MS で集約し、HOT_WINDOW_SECS 以内のファイルを HOT_POLL_SECS ごとに確認、RECONCILE_SECS ごとに走査。あふれ（`IN_Q_OVERFLOW`、`max_user_watches` 超過、ReadDirectoryChangesW のあふれ）では即時照合。失敗は §3.3 のバックオフ。時刻は注入可能な Clock 経由（試験用）。
- Aggregator: `sessionUid` で 1 つの表にまとめ、影響を受けたプロジェクトだけ再計算し、REEVAL_SECS ごとに全体を再評価、変化時だけ `projects_changed` を送る。
- Project Resolver（共通）: `file://` をデコードし比較キーと表示文字列を分ける。Windows キーはドライブ文字大文字・`\` 区切り・末尾区切りなし・全体小文字化（F18）。Linux キーは大文字小文字を保ち、`/mnt/<drive>/…` は Windows キーへ。`.git` を RESOLVE_MAX_DEPTH 階層まで上方探索して `git_root`（git は起動せずキャッシュ）、無ければ `raw_path`。cwd がログに無いときだけ `projects=[]`。

## 9. UI と安全

- 主画面は 1 行 1 プロジェクト、§4 の順。名前、「更新中」（low は「更新中?」）または相対時刻（`file_mtime` 由来は `~`）、エージェント。WSL は `(WSL: <distro>)`、`originAvailable=false` は「停止中」。Health は形式変更を 0 件で黙らせない。
- 本文は React のテキストノードだけ（`dangerouslySetInnerHTML`・外部リソース禁止、CSP `default-src 'self'`）。capability は `core:default` とイベント受信のみ。
- 子プロセスは必ず `.stdin(Stdio::null())`（F9）。

## 10. プライバシー

- 本文・ツール引数・出力はディスクに置かない。切り詰め（TRUNC_TEXT_CHARS、TRUNC_OUT_CHARS）と redaction は Rust 側で IPC 送信前に適用する。
- redaction は固定ルール、一致部分を `[REDACTED]` に置換（長さは §2）: R1 `\bsk-[A-Za-z0-9_-]{SK_MIN,}`、R2 `\bgh[pousr]_[A-Za-z0-9]{GH_LEN,}`、R3 `\bAKIA[0-9A-Z]{AWS_LEN}\b`、R4 `(?i)\b[A-Z0-9_]*(R4_WORDS)[A-Z0-9_]*\s*[=:]\s*\S+` の値部分（名前に語を含めば広く伏せる）。
- 元ログ時点で U+FFFD の CP932 メッセージは修復しない。診断出力（ヘルパー stderr 含む）は件数・コード・版番号のみ。fixture は合成値。

## 11. 故障モード対応と受け入れ試験（1 故障 = 1 機構 = 1 試験。pass は期待値との完全一致）

| ID | 故障モード | 機構 | fixture 入力 → 期待出力 |
|---|---|---|---|
| F1a | fork の継承 timestamp が fork 時刻に書換 | ordinal 未満の行を除外 | 行 0–2 の ts=F、ordinal=3、行 3 の ts=F+5s → `lastActivityAt`=F+5s |
| F1b | fork の ordinal 欠落 | 全取込 + `fork_boundary_unknown` + low | ordinal 無し・`session_meta` 2 つ → 診断 1 件、confidence=low |
| F2 | subagent が `end_turn` 無しの `tool_use` で終わる | 親ターン終了後は子を idle | 親 `end_turn` T+100、子最終 `tool_use` T+50、now_ms=T+110 → 親 `{waiting_input, high, stale=false}`、子 `{idle, low, stale=false, reason="parent turn ended after last activity"}`、親 `updating=false` |
| F3 | 巨大 `api-request-blob` | `type` を先に probe | 通常行の間に 3 MiB の blob（ts=T+1000）→ blob の完全 parse 0 回、`lastActivityAt` は最後の通常行 |
| F4 | 不正 UTF-8 | 行を捨て `invalid_utf8` | 0x82 0xA0 を含む行を正常 2 行の間に → `invalid_utf8`=1、前後は反映 |
| F5 | MAX_LINE_BYTES 超の行 | 捨てて `oversized_line` | §3.3 の境界値（内容 4194304 B / 4194305 B、改行有無）+ 後続の正常行 → 前者は処理、後者は破棄で `oversized_line`=1、後続は読める |
| F6 | 書き込み途中の末尾行 | 完全な行の末尾でだけ offset を進める | 改行無しの行、のちに改行追記 → 追記前はイベント無し・offset 不変、追記後に 1 件 |
| F7 | ファイル置換・縮小 | fileId・サイズで検出 | fileId 変更 / 縮小 → `file_replaced`。3 KB ファイルへ 100 B 追記 → 再読込しない |
| F8a | notify の欠落（追記） | HOT_POLL_SECS の確認 | 判定: `cargo test -p lens-core --test change_detector hot_poll`（exit 0）。fake Clock、notify 無効、追記後 Clock を 1 秒進める → 検出 0 回、2 秒 → 1 回。実時間待ちなし |
| F8b | notify の欠落（新規ファイル） | RECONCILE_SECS の走査 | 同 `--test change_detector reconcile`。fake Clock 59 秒 → 0 回、60 秒 → 新規 1 件 |
| F8c | notify のあふれ | 即時照合 | 同 `--test change_detector overflow`。あふれ模擬 → Clock を進めず照合 1 回 |
| F9 | 子プロセスの stdin 継承停止（`git` が止まり数十分 running のままだった） | `Stdio::null()` | 試験子（stdin を EOF まで読む）を、書かず開いたままの親パイプ下で起動。ready 後に fake Clock を 10 秒進め、その時点で exit 0 かつ読んだ byte 数 0 → pass、未終了 → fail。実時間待ちなし |
| F10a | WSL 時計のずれ | §3.2 の補正 | offset_ms=4999 / 5000 / 5001 / −5001 → なし / なし / あり / あり。§3.2 の数値で往復一致 |
| F10b | 補正後の未来時刻 | `now_ms` への丸め | t=now_ms+1s → `lastActivityAt`=now_ms、`clock_skew` |
| F11a | ヘルパー無応答 | HB_TIMEOUT_SECS で再起動 | heartbeat を 29 / 30 秒止める → 再起動なし / あり |
| F11b | 再起動の連続失敗 | §3.3 のバックオフ | 失敗 8 回 → 待ち 1, 2, 4, 8, 16, 32, 60, 60 秒。成功後の失敗 → 1 秒 |
| F11c | stale 切替の観測遅延 | REEVAL_SECS の再評価 | fake Clock で age_s=599 から 1 秒ずつ進め 5 秒周期で再評価 → 初観測は 600〜604（遅れ 0〜4 = REEVAL_SECS−1）、605 以降に持ち越さない |
| F12 | ディストロ停止 | 起動せず最後の状態を保持 | 停止を模擬 → `originAvailable=false`、`updating=false`、`updatingConfidence` なし、`lastUpdatedAt` 不変。SourceHealth.state=`distro_stopped`。`wsl.exe -d` 起動 0 回 |
| F13a | 版不一致 | 停止、再起動しない | hello の protocol=2 → `helper_version_mismatch`、再起動 0 回 |
| F13b | 必須欠落 | 行を捨てる | `heartbeat` に `now` 無し → `invalid_message`=1 |
| F13c | 未知 type | 無視 | `type="foo"` → `unknown_message`=1 |
| F13d | 巨大行 | 捨てる | 5 MiB の行 → `oversized_line`=1 |
| F13e | 壊れた JSON | 捨てる | `{"type":` → `malformed_record`=1 |
| F13f | hello 前の upsert | 捨てる | 最初の行が `session_upsert`（または `source_health`）→ `invalid_message`=1、セッション 0 件 |
| F14 | 承認待ち・長時間コマンド・クラッシュの区別 | **守れない**。`running` + `stale=true` で表示し「更新中」に数えない | tool_use 未解決で now_ms=T+700 → running, stale=true, updating=false |
| F15 | 自己観測（Lens 自身のリポジトリが更新中に出る） | **守れない**。識別変数が届かず除外しない | Lens 自身のセッションが running → 一覧に出る |
| F16a | `sk-` 鍵の混入 | R1 | `sk-`+24 文字 → `[REDACTED]`。`sk-`+19 文字 → 変化なし |
| F16b | GitHub トークンの混入 | R2 | `ghp_`+36 文字 → `[REDACTED]`。35 文字 → 変化なし |
| F16c | AWS キー ID の混入 | R3 | `AKIA`+16 文字 → `[REDACTED]`。15 文字 → 変化なし |
| F16d | 名前付き秘密の代入 | R4 | `ANTIGRAVITY_CSRF_TOKEN=abc` → `ANTIGRAVITY_CSRF_TOKEN=[REDACTED]`。`TOKEN_COUNT=5` → `TOKEN_COUNT=[REDACTED]` |
| F17 | Codex の二重発言 | `response_item` のみ本文に使う | 同一発言の `response_item` と `event_msg` → 1 件 |
| F18 | Windows パスの大小文字違いの誤統合 | **守れない**。小文字キーで比較 | `C:\Work\Repo` と `c:\work\repo` → 同一キーで 1 行 |
| F19 | guardian_review による更新の偽装 | automated を集計・活動から除外 | 親より新しい ts の guardian rollout → 親の `lastUpdatedAt` 不変、`autoReview=true` |
| F20 | 古い running が「更新中」に残る | stale は `updating` に数えない | now_ms=T+599 / 600 / 601 → updating は true / false / false |
| F21 | 初回走査の過負荷 | INITIAL_SCAN_DAYS | mtime が now_ms−7 日ちょうど / 1 秒前 → 読む / 読まず `skippedOldFiles`+1 |
| F22 | 元ログへの書き込み | 書き込みハンドルを開かない | 走査前後に各 fixture へ `Get-FileHash -Algorithm SHA256`（WSL 側は `sha256sum`）を実行し、前後の値と mtime を文字列比較。読み取り専用・読めないファイルも含める → 全ファイルで hash・mtime 一致、読めないファイルは `access_denied` |
| F23 | `\wsl.localhost` 越しの読み取り | UNC パスを root に受けない | `set_source_root` に `\\wsl.localhost\Ubuntu\home\u` → `path_invalid` |
| F23a | WSL 側 cwd `/mnt/c/x` の分裂 | Windows キーへ変換 | cwd `/mnt/c/x` と `C:\x` → 1 行 |
| F23b | Windows 側 `\wsl.localhost` cwd の分裂 | `wsl:` キーへ文字列変換 | `\wsl.localhost\Ubuntu\home\u\p` と WSL 側 `/home/u/p` → `wsl:Ubuntu:/home/u/p` の 1 行 |
| F24 | 未知の `thread_source` | 診断と `partial` | `thread_source="foo"` → `hierarchy.kind=top_level`、diagnostics に `{code:"unknown_thread_source",count:1}`、`health.completeness="partial"` |
| F25a | 不正な IPC 引数 | §8 のエラー code | 存在しない key / 空 key → `project_not_found` / `invalid_argument` |
| F25b | 同時刻のプロジェクト | §4 の名前昇順 | 同時刻の 2 プロジェクト → 名前の昇順 |
| F25c | 起動直後の get_health | `not_ready` | 起動前 → `not_ready`、初回走査中 → `SourceHealth[]` |
| F26 | 設定の保存失敗 | 変更を適用せず `settings_save_failed` | settings.json のディレクトリを読み取り専用にして `set_distro_enabled(Ubuntu,false)` → `settings_save_failed`、`disabledDistros` 不変 |
| F27 | 設定の永続化 | 一時ファイル + rename、起動時に読み込み | `set_distro_enabled(Ubuntu,false)` と `set_source_root(claude-local,"C:\x")` → settings.json は `{"version":1,"disabledDistros":["Ubuntu"],"sourceRoots":{"claude-local":"C:\\x"}}`。作り直した Settings が同値。`path=null` → `sourceRoots` から消える |
| F28 | 設定ファイルの破損 | 既定値起動 + 退避 | 不正 JSON → 既定値、`settings.json.bad` が存在、`settings_load_failed`=1。`helperPath="usr/bin/x"` → HELPER_PATH を使用 |
| F29a | ヘルパー CLI の引数不正 | 終了 2、再起動しない | `serve`（`--protocol` 欠落）/ `--roots rel` / 未知の引数 → 各々 exit 2・`usage_error`。Lens は `helper_unavailable`（detail `usage_error`）、再起動 0 回 |
| F29b | プロトコル不一致 | 終了 3、再起動しない | `serve --protocol 2` → exit 3・`protocol_unsupported`。Lens は `helper_version_mismatch`、再起動 0 回 |
| F29c | 全 root 不可 | 終了 4、バックオフ再起動 | `--roots /no/such` → exit 4・`root_unreadable`。再起動までの待ち 1 秒 |
| F29d | stdout 書き込み失敗 | 終了 5、バックオフ再起動 | stdout を閉じて起動 → exit 5・`stdout_failed`。待ち 1 秒 |
| F29e | panic | 終了 101、バックオフ再起動 | 試験用 feature `fault_inject` で panic → exit 101・`internal`。待ち 1 秒、再起動後に hello から再送 |
| F30 | 本文の切り詰め | Rust 側で IPC 前に TRUNC_TEXT_CHARS / TRUNC_OUT_CHARS | 発言 TRUNC_TEXT_CHARS / +1 文字、ツール出力 TRUNC_OUT_CHARS×2 / +1 文字 → 発言は全文 / 先頭 TRUNC_TEXT_CHARS 文字、出力は全文 / 先頭と末尾各 TRUNC_OUT_CHARS 文字。IPC payload とディスクに全文なし |
| F31 | Lens 側の二重 redaction | ヘルパーが漏らしても IPC 前に伏せる | redaction しない fake ヘルパーが `ghp_`+36 文字を含む `session_upsert` を送る → IPC payload は `[REDACTED]`、原文なし |
| F32 | settings の反映時点 | `helperPath` と `sourceRoots` は次のヘルパー起動、`disabledDistros` は次の DISTRO_POLL_SECS | 稼働中に `helperPath=/opt/h` を保存 → 稼働中は変更なし、再起動後の起動コマンドが `/opt/h`。`disabledDistros` 追加 → DISTRO_POLL_SECS−1 秒では稼働、DISTRO_POLL_SECS 秒で停止 |

## 12. 防御のトレードオフ

| 機構 | 壊す正当なケース | 代替案 | 決定 | 限界 |
|---|---|---|---|---|
| 厳格 UTF-8 | 行内の生 CP932 バイト | 置換文字 / CP932 推定 | 拒否 + 記録 | その行を全て失う。最終行なら unknown |
| redaction R1〜R4 | `TOKEN_COUNT=5` 等の無害な代入も伏せる | 利用者定義ルール / 伏せない | 伏せる | 未知形式の秘密は漏れる |
| fork 除外（ordinal のみ） | ordinal 無しの fork は継承分を除けない | 親 rollout の `turn_id` 差分 | 記録（全取込 + low） | fork 直後に実際より新しく見える |
| guardian_review 除外 | 審査スレッドの活動が本当に進行中の作業 | 親と同様に集計 | 拒否（`autoReview` で示す） | 審査だけが長く動くと updating に出ない |
| STALE_SECS | 10 分超の長時間コマンドが「更新中」から外れる | tool_open なら無期限 running | 警告（`running (stale)`） | 承認待ちと区別不能（F14） |
| 子を親終了で idle | 親終了後も動く background subagent | 子の終端を待つ | 警告（idle + low） | 誤って idle に見える（終端不在が実態：80 件中 69 件） |
| 停止中は起動せず、ヘルパー経由のみ | 停止中の更新が見えない。ヘルパーを置けない WSL が見えない | `wsl.exe` で起こす / `\wsl.localhost` 越し | 拒否（`helper_unavailable` を Health に出す） | 稼働・配置まで古い状態または表示不能 |
| clockOffset 補正 | 本当に 5 秒超進んだ時計の WSL は補正で時刻が動く | 補正しない / 時刻同期を要求 | 警告（補正 + `clock_skew`） | 受信遅延が誤差に残る。外れ値は 2 個まで耐える |
| heartbeat 無応答で再起動 | 高負荷で 30 秒応答できない正常なヘルパーを再起動する | プロセス生存だけ見る | 記録（再起動 + 初回走査やり直し） | 再起動中は最大 BACKOFF_MAX_SECS 古い状態 |
| Job Object | Lens と別寿命で動かしたい共有ヘルパーが Lens 終了で止まる | 放置 / 切断検知のみ | 拒否（共有しない） | Lens 強制終了後、ヘルパーは次の stdout 書き込み失敗まで残る |
| DEBOUNCE_MS | 200 ms 以内の変化が最大 200 ms 遅れて反映される | 即時処理 / 長い窓 | 記録 | 欠落は HOT_POLL_SECS・RECONCILE_SECS で補う。窓を長くすると遅延が増える |
| settings の一時ファイル + rename | 別プロセスの同時書き込み・rename 不可の FS | 直接上書き | 記録 | 最後の書き込みが勝つ。tmp が残り得る |
| NDJSON の不正行の破棄 | 不正行が重要な upsert だった | 停止 / 再起動 | 拒否（捨てて診断を記録） | 1 行分の更新を失い次の差分まで古い |
| Lens 側の二重 redaction | 同じ規則なので片方が漏らせば両方漏れる | ヘルパーのみ | 記録 | 規則外の秘密は漏れる。CPU は 2 倍 |
| 子の `Stdio::null()` | 対話の資格情報入力を要する子が失敗する | 継承 / パイプ | 拒否 | 失敗として扱い待たない |
| HOT_POLL_SECS の定期確認 | hot ファイルが数千件だと stat が負荷になる | notify のみ / 周期を延ばす | 記録（HOT_WINDOW_SECS 内に限定） | 毎秒 N/HOT_POLL_SECS 回の stat。検出は最大 HOT_POLL_SECS 遅れ |
| INITIAL_SCAN_DAYS | 古いが長く休止後に再開するログの履歴が見えない | 全件走査 / 日数を延ばす | 拒否（読まず `skippedOldFiles` に数える） | 追記で mtime が更新されるまで見えない |
| TRUNC_TEXT_CHARS / TRUNC_OUT_CHARS | 上限超の発言・出力の中間が見えない | 全文保持 | 記録（切り詰めて送る） | 全文検索は不可 |
| Windows パスキーの小文字化 | 大小文字で別物の `C:\Work\Repo` と `c:\work\repo` が 1 行に統合される（F18） | 大小文字を保つ | 記録 | 区別するディレクトリは統合される |

範囲外: Cursor / multi_root・alias・linked worktree 統合 / tokens / トレイ・トースト・単一インスタンス / 多世代ツリー詳細 UI / hooks 連携 / Version=2 以外の WSL / ネットワーク上の DB / Cloud Agent 履歴 / 公式 export 形式 / ヘルパーの配布・同梱・インストール / ビルド・CI / 運用
