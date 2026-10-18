# Agent Session Lens アーキテクチャ設計書 v2

v1（[architecture.md](architecture.md)）を、mcp-server-bm25-code-search のセッション検索（session recall）実装で
4製品のローカルログを実際に読み込み・検証した知見で改訂したもの。
根拠: 同リポジトリの `docs/research/session-recall/t0-report.md`（形式調査）、`implementation-report.md`（実装検証）、
`usefulness-eval-2026-10-17.md`（実ログ評価）、`bm25_search/sessions/adapters/*.py`（実装）。

## 0. v1からの主な変更点

| # | v1 | v2 | 理由（実測・実装で判明） |
|---|---|---|---|
| 1 | AGY/Cursor の SQLite を `mode=ro&immutable=1` で常時接続 | 通常は `mode=ro` + `query_only` + 短いread transaction。immutableはWAL/journalが無く、前後でhash不変の履歴DBだけの例外 | immutableは「ファイルが変わらない」ことを呼び出し側が保証する契約。稼働中のWAL付きDBに使うと未コミット/古い状態を読む・破損扱いになる |
| 2 | 「WAL存在時もライターを一切ブロックしない」 | read transactionを長く保持しない。保持中はcheckpointが進まずWALが肥大化する | ro接続でも `-shm` を扱う。sandbox環境ではSHMを開けず読めないDBがあった |
| 3 | AGY保存領域は `antigravity-ide` / `antigravity-cli` の2つ | 旧 `~/.gemini/antigravity` を加えた3領域を別sourceとして扱う | 3領域とも同一schemaの会話DBを確認 |
| 4 | `cwd` をそのままプロジェクトとして表示 | Project Resolver で正規化・Gitルート解決してからグルーピング | サブディレクトリのcwd、`file://` URI、複数root workspace、帰属不明ログが実在 |
| 5 | Claude の `type=user` 最新レコードを「直近のユーザー発話」 | tool_result と自動挿入タグを除いた人間の発言だけ | `type=user` envelope にtool結果が入る。タイトルが `<ide_opened_file>` 等で汚れる実例 |
| 6 | サブエージェントは「親子関係」とだけ記載 | 製品別の識別規則を明記（Claude: 同じsessionIdで `agentId` が別、Codex: `parent_thread_id`/fork継承範囲） | Claude subagentは親と同じsessionIdを使う実例を確認 |
| 7 | Codex の全セッションを一覧化 | `thread_source=guardian_review`（承認判定スレッド）は自動処理として非表示・別集計 | 親の会話の写しで、実ログ評価でノイズの約12%を占めた |
| 8 | `status: running/waiting_input/idle/error` | 推定根拠と確度を併記し、`unknown`（未対応形式・partial）を追加 | 状態はログからの推定。未知のstep/blockが一定数ある |
| 9 | 取得データを「取得可能」と一律に記載 | completeness（complete/partial/unsupported）と診断コードをUIまで運ぶ | transcriptの切り詰め、Cursorの時刻欠落・孤立composerなど欠落が常態 |
| 10 | 文字コードの考慮なし | UTF-8厳格デコード、失敗・置換文字を診断として表示 | 取り込んだタイトル・本文に `�` 混入を実測 |

---

## 1. 概要

AGY (Antigravity)、Claude Code、Codex CLI、Cursor のローカルセッションログを **読み取り専用・非破壊** で監視し、
「どのエージェントが・どのプロジェクトで・何をしているか」をリアルタイムに一覧化する Windows デスクトップダッシュボード。

### 設計原則

1. **元ログに書き込まない**: JSONL は共有読み取り、SQLite は ro 接続のみ。書き込み可能接続へのフォールバックは設けない。読めなければ `unavailable` を表示する。
2. **推測で帰属・状態を確定しない**: プロジェクトはログ内のメタデータから決める。本文中のファイルパスから推測しない。状態は根拠付きの推定値として表示する。
3. **欠落を隠さない**: 未対応の形式や欠けたデータは、空のまま成功扱いにしない。partial/unsupported と診断コードで表示する。
4. **形式はバージョンではなく実レコードで判定する**: インストールされた CLI の版とログを書いた版（writer version）は一致しない（例: Claude CLI 2.1.296 に対し JSONL の version は 2.1.300）。
5. **会話本文は永続化しない**: 本文はメモリ上の直近ウィンドウだけに保持する。永続化するのはオフセット・fingerprint・集計値に限る（§7）。

```mermaid
flowchart TD
    subgraph Storage [エージェントローカルストレージ（読み取り専用）]
        A1["AGY ×3領域 (cli / ide / legacy)<br/>conversations/*.db (+ -wal) + brain/<id>/…/transcript.jsonl"]
        A2["Claude Code<br/>$CLAUDE_CONFIG_DIR/projects/**/*.jsonl"]
        A3["Codex<br/>$CODEX_HOME/sessions/**/rollout-*.jsonl"]
        A4["Cursor<br/>globalStorage/state.vscdb + workspaceStorage/*/{state.vscdb,workspace.json}"]
    end

    subgraph Core [Core (Tauri v2 / Rust)]
        W["Change Detector<br/>notify (ReadDirectoryChangesW) + Debounce + 定期再走査"]
        Q["Ingestion Scheduler<br/>sourceごとの直列キュー / 優先度"]
        subgraph Readers [Readers]
            R1["JSONL Tail Reader<br/>byte offset + 不完全行バッファ"]
            R2["SQLite Snapshot Reader<br/>mode=ro + query_only + 短命transaction"]
        end
        subgraph Adapters [Session Adapters（形式判定・partial診断付き）]
            AD1["AGY: trajectory-v0 (step_format=0)"]
            AD2["Claude: jsonl-v2"]
            AD3["Codex: rollout-v1"]
            AD4["Cursor: storage-v1"]
        end
        PR["Project Resolver<br/>URI正規化 / Gitルート / alias / 帰属不明"]
        AGG["Session Aggregator<br/>階層化 / 自動スレッド除外 / 重複排除"]
        ST["Status Evaluator<br/>状態 + 根拠 + 確度"]
        STATE[("State Store<br/>メモリ + オフセット/fingerprintのみ永続化")]
    end

    subgraph UI [Dashboard (WebView2 / React)]
        D1["Agent Cards"]
        D2["Project Matrix"]
        D3["Live Inspector"]
        D4["Health / Diagnostics"]
    end

    Storage -->|ファイルイベント| W
    W --> Q
    Q --> R1 & R2
    R1 --> AD2 & AD3
    R2 --> AD1 & AD4
    R1 -. transcript補助 .-> AD1
    Adapters --> PR --> AGG --> ST --> STATE
    STATE -->|状態差分 / IPC| UI
```

---

## 2. エージェント別ストレージ仕様と取得方式

既定パスはコード内で固定し、環境変数で上書きできるようにする。UIからの任意パス入力は「追加source」として別扱いにする。

| source_id | 既定パス (Windows) | 上書き |
|---|---|---|
| `claude-local` | `%USERPROFILE%\.claude\projects` | `CLAUDE_CONFIG_DIR\projects` |
| `codex-local` | `%USERPROFILE%\.codex\sessions` | `CODEX_HOME\sessions` |
| `agy-cli` / `agy-ide` / `agy-legacy` | `%USERPROFILE%\.gemini\antigravity-cli` / `antigravity-ide` / `antigravity` | — |
| `cursor-local` | `%APPDATA%\Cursor\User` | — |

### 2.1 Claude Code（jsonl-v2）

| 用途 | フィールド | 注意 |
|---|---|---|
| セッション | `sessionId` | 1ファイルに別sessionIdが混在したら `mixed_session_ids` として除外 |
| サブエージェント | `isSidechain=true` + `agentId` | **親と同じsessionIdを使う**。子のIDは `sessionId::agent:<agentId>`、parentは `sessionId` とする。agentIdが1つに決まらなければpartial |
| 人の発言 | `type=user` かつ `message.content` が文字列またはtext block | tool_resultを含むuser envelopeは人の発言ではない |
| 応答 | `type=assistant` の `content[].type=text` | `thinking` / `redacted_thinking` / `image` は表示本文にしない |
| ツール | `tool_use.{id,name,input}` ↔ `tool_result.{tool_use_id,content,is_error}` | idで対応させる |
| project / 時刻 | 各レコードの `cwd` / `timestamp` | ファイルの配置場所だけでprojectを決めない |

除外対象: `system`、`attachment`、API requestの複製、進捗や履歴のsnapshot。
ユーザー発話の先頭にある `<ide_opened_file>`、`<command-message>`、`<system-reminder>` などの自動挿入タグは、summary や lastUserMessage に使わない。
`/clear` や新規チャットを行うと新しいJSONLファイルになる（§5.4のセッション切り替え判定に使う）。

### 2.2 Codex CLI（rollout-v1）

| 用途 | フィールド | 注意 |
|---|---|---|
| セッション | **先頭の** `session_meta.payload.id`（または `session_id`） | fork時に後から出てくる親の `session_meta` で上書きしない |
| 親子 | `session_meta.parent_thread_id`、または2つ目以降のsession_metaのid | fork継承範囲（timestampがsession開始より前のrecord）は「このセッションの活動」に数えない |
| 自動スレッド | `session_meta.thread_source == "guardian_review"` | 承認判定用で、親の会話の写し。一覧に出さず、親カードに「承認判定中」として集約する |
| project | 最初の `session_meta.payload.cwd` | turn_contextは補助情報 |
| 発言 | `response_item` / `payload.type=message` / `role=user,assistant` | **`event_msg` にも同じ発言があるため、両方を取り込むと二重になる**。response_itemを正とする |
| ツール | `custom_tool_call` / `function_call` ↔ `*_output`（`call_id`） | `output` は文字列とtext block配列の両方がある |

除外対象: `reasoning`、`encrypted_content`、developer/system の常設指示（AGENTS.md の挿入ブロックなど）。

### 2.3 AGY / Antigravity（trajectory-v0）

- 3領域それぞれで `conversations/<id>.db` と `brain/<id>/.system_generated/logs/transcript.jsonl` を対応させる。
- **projectは SQLite の `trajectory_metadata_blob` から取る**（field 7 の workspace_uris と、field 1 の workspace 内 field 1）。transcriptやCLIのcacheを唯一の根拠にしない。
- セッションIDは `trajectory_meta.cascade_id`。無ければDBファイル名を使う。
- `steps.step_payload` は step 全体を包む protobuf。既知の形式は `step_format=0` だけ。wrapperの field 1 が `steps.step_type` と一致し、field 5 が `steps.metadata` と一致することを毎回確認する。一致しなければその step を `unsupported_step_format` にする。

| step_type | 内容 | 本文の位置 |
|---|---|---|
| 14 | user input | wrapper 19 → 2 (user_response)、無ければ 1。items(3) だけの入力は未復号として partial |
| 15 | planner response | wrapper 20 → 8 (modified_response)、無ければ 1。3 (thinking) は除外。7 = tool_calls |
| 21 | RUN_COMMAND | wrapper 28 → 23 command_line、2 cwd、6 exit_code（符号付きに変換）、21 出力（full=1 / truncated=2） |
| その他tool | transcriptの `content` で補う | `truncated_fields` があれば切り詰めとして表示。`transcript_tool_fallback` 診断を付ける |

- step metadata の field 1 が created_at、field 4 が tool_call（field 1=id、2=name）。transcriptとは `step_index` で結合する。
- transcriptは切り詰められている（短縮版）。DBの本文を transcript で上書きしない。
- tool call ID がない場合、順序だけで call と result を対応させない。

### 2.4 Cursor（storage-v1）

- **project帰属**: `workspaceStorage/<wsId>/workspace.json` の `folder`、または `workspace`/`configuration`（`.code-workspace` を解決して複数rootになる）。
  - 新しい経路: global DB の `composerHeaders(composerId, workspaceId)`。
  - 旧い経路: workspace DB の `ItemTable['composer.composerData'].allComposers[].composerId`。
  - 実測では header 27件のうち workspaceStorage と一致したのは15件だった。一致しない composer は「帰属不明」として表示し、現在開いているプロジェクトに割り当てない。
- **本文**: global DB の `cursorDiskKV`。
  - `composerData:<cid>` の `conversation[]` に本文を持つ inline 形式。
  - `fullConversationHeadersOnly[].bubbleId` から `bubbleId:<cid>:<bid>` を引く外部形式。
  - 値が NULL や参照先欠損のものは tombstone として扱う（実測では bubble 2,963件中83件が NULL）。
- `type=1` は human、`type=2` は AI。`isThought` / allThinkingBlocks は除外する。`toolFormerData` からツール情報を取る。params/result は常に JSON とは限らない。
- **時刻**: bubble の createdAt は欠けていることが多い（実測で138件のみ）。その場合 lastActiveAt は composerHeaders の lastUpdatedAt、それも無ければ DB 更新時刻で補い、補った値であることを表示する。session の時刻を各 bubble に複製しない。

---

## 3. 技術スタック

**採用: Tauri v2（Rust + React / Tailwind CSS + TypeScript）**。v1 の選定理由は維持する。v2 で追加・修正する点は以下。

- SQLite: `rusqlite`（bundled）。URI `file:...?mode=ro` で開き、`PRAGMA query_only=ON` を実行する。
- protobuf: AGY の既知 field 番号だけを読む小さな wire デコーダを自作する（スキーマ生成に依存しない）。未知の wire type や長さの不整合は拒否する。
- 監視: `notify` に加えて、定期的な再走査（例: 60秒ごと）で取りこぼしを回収する。ReadDirectoryChangesW はバッファがあふれるとイベントを落とす。
- 「Electron の1/5」というメモリ値は目標値とする。SQLite と全セッションの状態を保持した状態で実測して確認する。

---

## 4. 共通セッションモデル

```typescript
export type Agent = "agy" | "claude_code" | "codex" | "cursor";
export type Completeness = "complete" | "partial" | "unsupported" | "unavailable";

export interface UnifiedSession {
  sessionUid: string;            // `${sourceId}:${nativeId}` 。製品間・領域間で衝突しない
  nativeId: string;              // Claude subagent は `${sessionId}::agent:${agentId}`
  agent: Agent;
  sourceId: string;              // claude-local / agy-ide など
  writerVersion?: string;        // ログを書いた版。CLIの版とは別物

  project: {
    key: string | null;          // 正規化済みのprojectルート。帰属不明なら null
    name: string;
    rawPaths: string[];          // ログ上の cwd / workspace URI（正規化前）
    attribution: "metadata" | "git_root" | "alias" | "multi_root" | "unknown";
  };

  status: {
    value: "running" | "awaiting_approval" | "waiting_input" | "idle" | "error" | "unknown";
    reason: string;              // 例: "unresolved tool_use: Bash (id=…)"
    confidence: "high" | "low";  // 製品ごとの判定能力に応じる（§5.3）
    since: string;
  };

  currentTask: {
    summary: string;             // 自動挿入タグを除いた最初の人間の発言
    lastUserMessage?: string;    // tool_result / 自動挿入を除いたもの
    activeTool?: {
      name: string;
      input: string;             // 表示用に切り詰め済み（§7）
      callId?: string;
      startedAt?: string;
    };
    lastExitCode?: number;       // AGY RUN_COMMAND など、取得できる製品だけ
  };

  metrics: {
    stepCount: number;
    tokens?: {                   // 出所を確認できた製品だけ値を入れる。不明なら undefined
      cachedInput: number;
      uncachedInput: number;
      output: number;
      source: string;            // 例: "claude:message.usage"（未検証の出所は表示しない）
    };
  };

  hierarchy: {
    parentUid?: string;
    childUids: string[];
    kind: "top_level" | "subagent" | "fork" | "automated";   // automated = guardian_review 等
    inheritedUntil?: string;     // Codex fork の継承範囲の終点
  };

  timestamps: {
    startedAt?: string;
    lastActiveAt?: string;
    lastActiveSource: "event" | "header" | "file_mtime";      // 時刻の出所
  };

  health: {
    completeness: Completeness;
    diagnostics: { code: string; count: number }[];          // 例: unknown_block, missing_bubble
  };
}
```

v1 からの変更点:
- `projectPath` は `project` オブジェクトに変更し、帰属の根拠を持たせた。
- `status` に根拠と確度を持たせた。
- tokens は任意項目にした。0 と「取得できない」を区別するため。
- `health` を新設した。

---

## 5. コアコンポーネント設計

### 5.1 Readers（読み取り専用の取り込み）

**JSONL Tail Reader（Claude / Codex / AGY transcript）**
- 共有読み取り（`FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`）で開く。DELETE 共有を付けないと、エージェント側のローテーションや削除を妨げる。
- ファイルごとに `(byte offset, size, mtime, 先頭4KBのhash)` を保持する。
  - ファイルが小さくなった、または先頭hashが変わった場合は、置き換えられたとみなして先頭から読み直す。
  - 改行で終わっていない末尾行はバッファに残し、次のイベントで続きを読む（`incomplete_tail`）。
  - 末尾以外の壊れた行は `malformed_record` としてスキップする。前後の正常な行は失わない。
- バイト列のまま UTF-8 として厳格にデコードする。失敗した行は診断として扱い、置換文字のまま表示しない。PowerShell 経由のパイプや CP932 を推定して読み直すことはしない。
- ファイルサイズの上限（既定64MiB）を超える場合、初回は末尾だけを読み、`truncated_initial_scan` を表示する。

**SQLite Snapshot Reader（AGY / Cursor）**
```
open  file:<path>?mode=ro      (timeout 2s)
PRAGMA query_only=ON
BEGIN                          -- 1回のスナップショット読み取りだけ
  SELECT …                     -- 必要なキーだけ
COMMIT / close                 -- すぐ閉じる。接続は保持しない
```
- read transaction を保持している間は WAL の checkpoint が進まず、エージェント側の WAL が肥大化する。常時接続やトランザクションの保持はしない。
- ro で開けなかった場合のフォールバックは、**`-wal` と `-journal` が存在しない場合に限り**行う。前後の fingerprint を比べ、`immutable=1` と `quick_check` で読む。読み終わった後に WAL が現れたり fingerprint が変わったりした場合、その結果は捨てる（`source_changed`）。
- それ以外の失敗は `unavailable` として表示する。書き込み可能な接続で再試行しない。
- 変更の検出には本体ではなく **`-wal` のイベント** を見る。WAL モードでは書き込みがまず `-wal` に入る。

### 5.2 Change Detector と Ingestion Scheduler

- 監視ルートは §2 の既定パスだけ。Cursor の globalStorage は頻繁に更新されるため、デバウンスを長めにする（例: 1〜2秒）。JSONL は短くする（例: 200ms）。
- source ごとに直列のキューを持つ。同じ DB への読み取りを並行させない。
- AGY は DB・WAL・transcript の3つを1つの成果物の依存ファイルとして扱う。どれかが変われば、その会話だけを再読み込みする。
- Cursor は `composerHeaders` と `composerData:*` の更新を手がかりに、変わった composer だけを読む。全 bubble の走査は初回だけにする。

### 5.3 Status Evaluator（根拠付きの状態推定）

| 状態 | 判定 | 確度 |
|---|---|---|
| `running` | 未解決の tool call がある（call はあるが result が無い）。または直近の数秒以内に追記がある | Claude / Codex: high、Cursor: low |
| `awaiting_approval` | 未解決の tool call が N 秒以上続き、ログの追記が止まっている | **low**（承認待ちと長時間実行中のコマンドは、ログでは区別できない） |
| `waiting_input` | 最後のイベントがアシスタントのテキスト応答で、未解決の call が無い | high |
| `idle` | 最終更新から 600 秒以上経過 | high |
| `error` | 直近の tool result が `is_error`、または exit_code が 0 以外で、その後の応答が無い | 製品依存 |
| `unknown` | 最新のイベントが未対応形式、または読み取り不能 | — |

- 600 秒は、session recall の「実行中セッション判定」で使っている閾値と同じ（`IDLE_SECONDS`）。
- Codex の guardian_review スレッドが動いている間は、親セッションを `awaiting_approval`（確度 high）に引き上げる。
- AGY の RUN_COMMAND は exit_code を持つ。それ以外の tool は transcript で補ったものなので、確度を下げる。

### 5.4 Session Aggregator

- **階層**: Claude の subagent（sidechain + agentId）、Codex の `parent_thread_id` と fork、guardian_review（automated）を `hierarchy.kind` で区別する。一覧の既定は top_level だけを表示し、子は親カードの中に入れ子にする。
- **セッション切り替え**: 同じプロジェクト・同じエージェントで、古いセッションのログが 600 秒以上更新されず、新しい top_level のログが更新されている場合、`/clear` や新規チャットで切り替わったとみなす。古い方は自動で `idle` にする。
- **重複排除**: Codex の event_msg と response_item、AGY の DB と transcript は、正とする stream だけを使う。本文が似ているだけの別の発言はまとめない。
- **自己観測の除外（任意）**: ダッシュボードを操作している側のセッションを特定できるのは、環境変数（`CLAUDE_CODE_SESSION_ID` / `CODEX_THREAD_ID`）が届く場合だけ。AGY は MCP などの子プロセスに `ANTIGRAVITY_CONVERSATION_ID` を渡さない。そのため常駐アプリからは特定できない前提で設計する。

### 5.5 Project Resolver

1. `file://` URI をパスに変換し、パーセントデコード、区切り文字の統一、Windows ではドライブ文字と大文字小文字を正規化する。
2. `git rev-parse --show-toplevel` に相当する上方探索で Git ルートを求め、それをプロジェクトのキーにする。結果はパスごとにキャッシュする。
   - サブディレクトリの cwd を別プロジェクトとして並べない。
   - `.git` が直下にあるかだけを確認する方式は誤る。サブディレクトリでは見つからない。
3. linked worktree は別プロジェクトとして扱う（表示上は同じリポジトリとしてまとめてもよい）。
4. Cursor の複数 root workspace や、AGY で複数の workspace_uris を持つ場合は `multi_root` とし、すべてのプロジェクト行に表示する。
5. どのルールでも決まらない場合は「帰属不明」の行にまとめる。本文中のパスから推測しない。
6. 移動前のパスとの対応は、ユーザーが明示した alias だけを使う。

---

## 6. ダッシュボード UI 構成

1. **Agent Cards（最上段）**: エージェントごとに稼働中セッションの数と、最も新しい活動を表示する。
   例: `[Claude Code] agent-session-lens: Running Bash 'npm test' (45s) · subagent ×2`
   確度が low の状態は `?` を付けて表示する（例: `承認待ち?`）。
2. **Project Matrix（メイン）**: 行に Git ルート、列にエージェントを並べる。行の末尾に「帰属不明」を常に置く。automated のスレッドは件数だけを表示する。
3. **Live Inspector（ドリルダウン）**: 直近のメッセージ、ツール履歴（call と result の対応、exit_code、切り詰めの有無）、親子ツリー、トークン推移（取得できる製品だけ）を表示する。
4. **Health / Diagnostics（新設）**: source ごとに次を表示する。
   - 最終読み取り時刻、`unavailable` の理由
   - partial になったセッションの数と、上位の診断コード
   - writer version の一覧
   - ログ形式が変わった場合、まずここに表示が出る。サイレントに 0 件にならないようにする。

---

## 7. プライバシーとリソースの上限

- 会話本文やツールの引数・出力は、ディスクにキャッシュしない。永続化するのは、オフセット・fingerprint・集計値・診断コードだけにする。
- 表示用の文字列は切り詰める。例: ツール入力 2,000 文字、出力の先頭と末尾 1,000 文字。元ログ側ですでに切り詰められている場合（transcript の `truncated_fields`、RunCommandOutput の truncated）は、それを明示する。
- redaction ルール（正規表現と置換文字列）を設定で指定でき、表示の直前に適用する。
- 診断のログ出力には、件数・診断コード・版番号だけを含める。本文・タイトル・パスは出力しない。
- メモリには、セッションごとに直近 N 件のイベントだけを保持する。古いセッションは集計値だけを残す。

---

## 8. テスト方針

- **合成 fixture**: 実際のログをコピーしない。構造だけを再現し、本文・ID・パスは合成値にする。AGY の protobuf は field 番号を保ったまま、値を合成して作る。
- **回帰テストの例**:
  - Claude subagent が親と同じ sessionId を持つ場合
  - Codex の fork 継承範囲と guardian_review
  - event_msg との二重取り込み
  - AGY の未知 step_format
  - Cursor の孤立 composer、NULL の bubble、時刻欠落
  - 不完全な末尾行、ファイルの置き換え
  - UTF-8 として不正なバイト列
  - WAL 付き DB で immutable にフォールバックしないこと
- **実機スモーク**: 明示したフラグを付けたときだけ実ログを読む。結果には集計値だけを出力する。
- テストを各エージェントのシェルから実行すると、`CLAUDE_CODE_SESSION_ID` などの環境変数を子プロセスが引き継ぐ。fixture で消しておく。

---

## 9. 未検証・初期対象外

- **トークン値の出所**: Claude の `message.usage`、Codex の token_count 系イベント、AGY の `gen_metadata` が候補。いずれも本プロジェクトでは形式を確認していないため、T0 調査を行うまで UI には出さない。AGY の usage 系テーブルは使用量であり、本文の代わりにはならない。
- **承認待ちの判定**: ログだけでは確定できない。各製品の hooks（AGY の Hooks は conversationId / workspacePaths を渡す）を補助に使う案は、将来の検討事項とする。
- **対象外**: Windows と WSL をまたいで同じ稼働中の DB を読むこと、ネットワーク上の WAL DB、Cursor の Cloud Agent のリモート履歴、各製品の公式 export 形式。
- **確認範囲**: 製品のすべての版での互換性は確認していない。確認したのは調査時点の版（例: Codex writer 0.162.0-alpha、Cursor 3.20.10）だけ。未知の形式は §6.4 の Health に表示する。
