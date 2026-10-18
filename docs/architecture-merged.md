# Agent Session Lens アーキテクチャ設計書（統合版）

v1（architecture.md）〜 v4（architecture-v4.md）を1つにまとめたもの。各版の差分を適用した後の、現時点で有効な設計だけを書く。

知見の出典:
- mcp-server-bm25-code-search のセッション検索（session recall）実装と、4製品の実ログでの検証。同リポジトリの `docs/session-recall.md`、`docs/research/session-recall/t0-report.md`（形式調査）、`implementation-report.md`（実装検証）、`usefulness-eval-2026-10-17.md`（実ログ評価）、`bm25_search/sessions/adapters/*.py`（実装）、コミット `7418591`、`ce7d6d1`。
- 合議レビュー [docs/gougi/2026-10-17-architecture-v1-v2-review.md](gougi/2026-10-17-architecture-v1-v2-review.md)。
- 2026-10-17 に実ログの構造（キーと型だけ）と、各製品の MCP プロセスの環境変数を再調査した結果。
- [kenn-io/agentsview](https://github.com/kenn-io/agentsview)（コミット `33c30f4`）のパーサ実装（`internal/parser/`）。本文中で「agentsview」と書いた知見は、このコードからの読み取りで、本プロジェクトでは実ログで確認していない。

---

## 1. 概要と設計原則

AGY (Antigravity)、Claude Code、Codex CLI、Cursor のローカルセッションログを **読み取り専用・非破壊** で監視し、
「どのエージェントが・どのプロジェクトで・何をしているか」をリアルタイムに一覧化する Windows デスクトップダッシュボード。

### 設計原則

1. **元ログに書き込まない**: JSONL は共有読み取り、SQLite は ro 接続のみ。書き込み可能接続へのフォールバックは設けない。読めなければ `unavailable` を表示する。
   - 会話データの論理的な内容を変更しない。書き込み可能な接続やファイルハンドルを開かない。
   - SQLite の ro 接続は、WAL の読み取りのために `-shm` を共有メモリとして扱うことがある。これは SQLite の仕様の範囲として許容する。`-shm` を開けなければ `unavailable` にする（sandbox 環境で SHM を開けず読めない DB があった）。
2. **推測で帰属・状態を確定しない**: プロジェクトはログ内のメタデータから決める。本文中のファイルパスから推測しない。状態は根拠付きの推定値として表示する。
3. **欠落を隠さない**: 未対応の形式や欠けたデータは、空のまま成功扱いにしない。partial/unsupported と診断コードで表示する。
4. **形式はバージョンではなく実レコードで判定する**: インストールされた CLI の版とログを書いた版（writer version）は一致しない（例: Claude CLI 2.1.296 に対し JSONL の version は 2.1.300）。
5. **会話本文は永続化しない**: 本文はメモリ上にだけ保持する。永続化するのはオフセット・fingerprint・集計値に限る（§7）。

```mermaid
flowchart TD
    subgraph Storage [エージェントローカルストレージ（読み取り専用）]
        A1["AGY ×3領域 (cli / ide / legacy)<br/>conversations/*.db (+ -wal) + brain/<id>/…/transcript.jsonl"]
        A2["Claude Code<br/>$CLAUDE_CONFIG_DIR/projects/**/*.jsonl"]
        A3["Codex<br/>$CODEX_HOME/sessions/**/rollout-*.jsonl"]
        A4["Cursor<br/>globalStorage/state.vscdb + workspaceStorage/*/{state.vscdb,workspace.json}"]
    end

    subgraph Core [Core (Tauri v2 / Rust)]
        W["Change Detector<br/>notify + ホットファイルポーリング + 60秒照合"]
        Q["Ingestion Scheduler<br/>dirty 集合 / 新しい順 / 周期あたり上限"]
        subgraph Readers [Readers]
            R1["JSONL Tail Reader<br/>fileId + offset + 上限付き先頭/末尾読み"]
            R2["SQLite Snapshot Reader<br/>mode=ro + query_only + 短命transaction + 会話単位の上限"]
        end
        subgraph Adapters [Session Adapters（形式判定・partial診断付き）]
            AD1["AGY: trajectory-v0 (step_format=0)"]
            AD2["Claude: jsonl-v2"]
            AD3["Codex: rollout-v1"]
            AD4["Cursor: storage-v1"]
        end
        PR["Project Resolver<br/>URI正規化 / .git上方探索 / raw_path / 帰属不明"]
        AGG["Session Aggregator<br/>階層化 / 自動スレッド除外 / 重複排除"]
        ST["Status Evaluator<br/>ターン状態機械 + 根拠 + 確度"]
        STATE[("State Store<br/>メモリ + オフセット/fingerprintのみ永続化")]
        RED["Snapshot Builder<br/>切り詰め + redaction（IPC 送信前）"]
    end

    subgraph UI [Dashboard (WebView2 / React)]
        D1["Agent Cards"]
        D2["Project Matrix"]
        D3["Live Inspector"]
        D4["Health (SourceHealth)"]
    end

    Storage -->|ファイルイベント / ポーリング| W
    W --> Q
    Q --> R1 & R2
    R1 --> AD2 & AD3
    R2 --> AD1 & AD4
    R1 -. transcript補助 .-> AD1
    Adapters --> PR --> AGG --> ST --> STATE --> RED
    RED -->|状態差分 / IPC| UI
```

---

## 2. エージェント別仕様

既定パスはコード内で固定し、環境変数で上書きできるようにする。UI からの任意パス入力は「追加 source」として別扱いにする。

| source_id | 既定パス (Windows) | 上書き |
|---|---|---|
| `claude-local` | `%USERPROFILE%\.claude\projects` | `CLAUDE_CONFIG_DIR\projects` |
| `codex-local` | `%USERPROFILE%\.codex\sessions` | `CODEX_HOME\sessions` |
| `agy-cli` / `agy-ide` / `agy-legacy` | `%USERPROFILE%\.gemini\antigravity-cli` / `antigravity-ide` / `antigravity` | — |
| `cursor-local` | `%APPDATA%\Cursor\User` | — |

### 2.1 Claude Code（jsonl-v2）

**ファイル配置**
- `projects/<encoded-cwd>/<sessionId>.jsonl`（トップレベル）
- `projects/<encoded-cwd>/<sessionId>/subagents/agent-<agentId>.jsonl`（subagent）。同じディレクトリの `*.meta.json` は読まない。
- `/clear` や新規チャットを行うと新しい JSONL ファイルが作られ、新しい sessionId になる。プロセスの環境変数 `CLAUDE_CODE_SESSION_ID` は **起動時の値のまま** 変わらない（§5.4）。

| 用途 | フィールド | 注意 |
|---|---|---|
| セッション | `sessionId` | 1ファイルに別 sessionId が混在したら `mixed_session_ids` として除外 |
| サブエージェント | `isSidechain=true` + `agentId`、または `subagents/` 配下のファイル | **親と同じ sessionId を使う**。子の ID は `sessionId::agent:<agentId>`、parent は `sessionId`。agentId が1つに決まらなければ partial |
| 人の発言 | 下記の判定規則 | tool_result を含む user envelope は人の発言ではない |
| 応答 | `type=assistant` の `content[].type=text` | `thinking` / `redacted_thinking` / `image` は表示本文にしない |
| ツール | `tool_use.{id,name,input}` ↔ `tool_result.{tool_use_id,content,is_error}` | id で対応させる |
| 終端シグナル | assistant の `message.stop_reason` | `end_turn` はターン完了、`tool_use` はツールの実行待ち |
| エラー | `type=system, subtype=api_error` | |
| project / 時刻 | 各レコードの `cwd` / `timestamp` | ファイルの配置場所だけで project を決めない |

**人の発言の判定**
- `type=user`、`isMeta` が無い、tool_result を含まない、の3つを満たす。
- text block ごとに、先頭の自動挿入タグ（`<ide_opened_file>`、`<ide_selection>`、`<system-reminder>`、`<browser_instruction>`、`<command-message>`、`<local-command-*>`、`<task-notification>` など）を取り除く。何も残らない block は使わない。
- `<command-name>X</command-name>` と `<command-args>Y</command-args>` は、`X Y` という人の発言として扱う。
- `[Request interrupted by user…` は中断のシグナルとして扱う（§5.3）。

除外対象: `system`（api_error 以外）、`attachment`、API request の複製（`api-request-blob`）、進捗や履歴の snapshot。

- **タイトル**（agentsview）: `type=custom-title` の `customTitle`（ユーザーが付けた名前。後のターンでも繰り返し書かれる）と、`type=ai-title` の `aiTitle`（自動生成）がある。title は custom-title、ai-title、最初の人間の発言の順に採る。
- **ストリーミングの分割**（agentsview）: 1つの応答が、同じ `message.id` を持つ連続した assistant レコードに分かれて書かれる（累積スナップショットと追加チャンクの両方がある）。同じ `message.id` の連続レコードは1つにまとめ、usage と metadata は最後のレコードのものを使う。まとめないと tokens が二重に数えられる。

### 2.2 Codex CLI（rollout-v1）

保存場所: `sessions/YYYY/MM/DD/rollout-<時刻>-<id>.jsonl`。

| 用途 | フィールド | 注意 |
|---|---|---|
| セッション | **先頭の** `session_meta.payload.id`（または `session_id`） | 2つ目以降の `session_meta` は fork 元のもの。それで上書きしない |
| 親子 | `parent_thread_id`、または `source.subagent.thread_spawn.parent_thread_id` | fork 元は `forked_from_id` |
| fork 継承範囲 | `session_meta.subagent_history_start_ordinal`（0始まりの行番号） | 下記参照 |
| 種類 | `thread_source`: `user`（top_level）、`subagent`、`guardian_review`（automated） | `hierarchy.kind` に使う |
| project | 先頭の `session_meta.payload.cwd` | turn_context は補助情報 |
| 発言 | `response_item` / `payload.type=message` / `role=user,assistant` | **`event_msg` にも同じ発言があるため、両方を取り込むと二重になる**。本文は response_item を正とする |
| assistant の段階 | message の `phase`: `commentary`（途中経過）、`final_answer`（最終回答） | |
| ツール | `custom_tool_call` / `function_call` ↔ `*_output`（`call_id`） | `output` は文字列と text block 配列の両方がある |
| ターンのライフサイクル | `event_msg` の `task_started` / `task_complete`（`error` を含む場合あり）/ `turn_aborted`（`reason` を含む） | event_msg は状態の判定にだけ使う |

**fork 継承範囲**
- 行番号が `subagent_history_start_ordinal` より小さいレコードは fork 元から継承したもので、このセッションの活動に数えない。継承されたレコードの timestamp は fork 時刻に書き換わっているため、timestamp では判定できない。
- ordinal が無いのに2つ目の session_meta がある場合は、継承範囲を推定しない。継承されたレコードには `task_started` も含まれるため、レコードの種類でも境界を決められない。全体を取り込み、`fork_boundary_unknown` の診断を付けて状態の確度を low にする。
- MVP 後の候補（agentsview の方式）: 親 rollout の `turn_context.turn_id` の集合を作り、子の `turn_context` のうち親に無い `turn_id` が最初に現れた位置を境界とする。それより前はすべて継承範囲。親 rollout を読める場合だけ使える。

**表示に使える補助情報**（先頭の `session_meta`）

| フィールド | 例 | 用途 |
|---|---|---|
| `originator` | `Codex Desktop` | クライアントの種類 |
| `source.subagent` | `{"other": "guardian"}`、`{"thread_spawn": {...}}` | guardian の判定の補助 |
| `agent_role`、`agent_nickname`、`agent_path` | `sg-verifier`、`Russell`、`/root/.../verifier_r1` | subagent のラベル（`hierarchy.label`） |

**guardian_review の扱い**
- 承認判定用の自動審査スレッドで、親の会話の写し。一覧に出さず、親カードに `autoReview`（「自動審査中」）として集約する（§5.3）。
- user 発言は「The following is the Codex agent history whose request action you are assessing…」で始まる。title・lastUserMessage・lastAssistantMessage・metrics に使わない。
- 親の作業中に、親と同じ秒で次々に作られる。「プロジェクトで最新のセッション」を選ぶ処理（§5.4）では、`thread_source=user` の top_level だけを候補にする。
- Project Matrix の件数に数えない。

**その他の手がかり**（agentsview）
- `originator=codex_exec` は `codex exec` による非対話実行。`thread_source` が無くても automated として扱える。
- `event_msg` の `collab_agent_spawn_end` は `call_id` と `new_thread_id` を持ち、親の tool call と子スレッドを結びつける。
- `CODEX_HOME/session_index.jsonl` の各行（`id`、`thread_name`）がスレッド名を持つ。リネームされるとこのファイルが更新される。title は thread_name を優先する。

**除外対象**: `reasoning`、`encrypted_content`、developer/system の常設指示。user 発言のうち、`<` で始まる block と `# AGENTS.md instructions` で始まる block は自動挿入として除外する。

### 2.3 AGY / Antigravity（trajectory-v0、MVP 対象外）

- 3領域それぞれで `conversations/<id>.db` と `brain/<id>/.system_generated/logs/transcript.jsonl` を対応させる。
- **セッション ID**: `conversations/<id>.db` のファイル名。`trajectory_meta.cascade_id` と全件一致した（2026-10-17、504件）。一覧はディレクトリの走査だけで作り、中身を読むのは変更があった会話だけにする。
- **活動時刻**: `mtime(<id>.db)` と `mtime(<id>.db-wal)` の新しい方。WAL モードでは書き込みがまず `-wal` に入るため。
- `presence/<id>.lock` は使わない。0 バイトで、終わった会話の分も残り続ける（cli で 341 件）。
- **project は SQLite の `trajectory_metadata_blob` から取る**（field 7 の workspace_uris と、field 1 の workspace 内 field 1）。transcript や CLI の cache を唯一の根拠にしない。
  - workspace URI が無い会話が多い。実測では cli の 349 件中 203 件が帰属不明だった。帰属不明として表示し、推測で割り当てない。
- `steps.step_payload` は step 全体を包む protobuf。既知の形式は `step_format=0` だけ。wrapper の field 1 が `steps.step_type` と一致し、field 5 が `steps.metadata` と一致することを毎回確認する。一致しなければその step を `unsupported_step_format` にする。

| step_type | 内容 | 本文の位置 |
|---|---|---|
| 14 | user input | wrapper 19 → 2 (user_response)、無ければ 1。items(3) だけの入力は未復号として partial |
| 15 | planner response | wrapper 20 → 8 (modified_response)、無ければ 1。3 (thinking) は除外。7 = tool_calls |
| 21 | RUN_COMMAND | wrapper 28 → 23 command_line、2 cwd、6 exit_code（符号付きに変換）、21 出力（full=1 / truncated=2） |
| その他 tool | transcript の `content` で補う | `truncated_fields` があれば切り詰めとして表示。`transcript_tool_fallback` 診断を付ける |

- step metadata の field 1 が created_at、field 4 が tool_call（field 1=id、2=name）。transcript とは `step_index` で結合する。
- transcript は切り詰められている（短縮版）。DB の本文を transcript で上書きしない。
- tool call ID がない場合、順序だけで call と result を対応させない。
- 実行中の会話 ID は、ツールが実行するシェルには `ANTIGRAVITY_CONVERSATION_ID` として渡される。`ANTIGRAVITY_TRAJECTORY_ID` は別の値で、会話 ID ではない。MCP サーバーには渡されない（§5.4）。

### 2.4 Cursor（storage-v1、MVP 対象外）

- 全プロジェクトの全会話が `globalStorage/state.vscdb` 1つに入る。軽く使っている PC でも 96MB あった。メインで使う PC ではさらに大きいと想定する。
- **project 帰属**: `workspaceStorage/<wsId>/workspace.json` の `folder`、または `workspace`/`configuration`（`.code-workspace` を解決して複数 root になる）。
  - 新しい経路: global DB の `composerHeaders(composerId, workspaceId)`。
  - 旧い経路: workspace DB の `ItemTable['composer.composerData'].allComposers[].composerId`。
  - 一致しない composer は「帰属不明」として表示し、現在開いているプロジェクトに割り当てない。実測では 161 件中、他プロジェクト 102、帰属不明 58（workspace が削除済みなど）、未対応形式 1。帰属不明が多い前提で UI を作る。
  - agentsview は workspaceStorage を経由せず、`composerData:<cid>` の `workspaceIdentifier.uri.fsPath`、無ければ `trackedGitRepos[0].repoPath` を cwd にしている。workspace が削除済みでも残るので、上の帰属不明 58 件を減らせる見込みがある（未実測）。この2つを workspaceStorage より先に試す。
- **composerHeaders の列**（実測）: `composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, checkpointAt, value, subagentTypeName`。
  - `lastUpdatedAt`（ミリ秒）を会話ごとの活動時刻にする。`state.vscdb` の mtime はどの会話が更新されても変わるので使わない。
  - ただし agentsview によると、`lastUpdatedAt` は bubble 自身の時刻より遅れることがあり、bubble の追加や書き換えで `lastUpdatedAt` が変わらないこともある。活動時刻は `lastUpdatedAt` と bubble の時刻の新しい方にする。変更判定は §5.2 のとおり、会話のキー範囲の digest で補う。
  - `isSubagent` / `subagentTypeName` で `hierarchy.kind=subagent` を表せる（親の composer ID の列は見つかっていない）。
  - `isArchived=1` は既定の一覧から外す。
- **本文**: global DB の `cursorDiskKV`。会話1件の中身は `composerData:<cid>` と `bubbleId:<cid>:*` の値で、サイズの上限と変更の判定はこの範囲で行う（§5.1）。
  - `composerData:<cid>` の `conversation[]` に本文を持つ inline 形式。
  - `fullConversationHeadersOnly[].bubbleId` から `bubbleId:<cid>:<bid>` を引く外部形式。
  - 値が NULL や参照先欠損のものは tombstone として扱う（実測では bubble 2,963件中83件が NULL）。
- `type=1` は human、`type=2` は AI。`isThought` / allThinkingBlocks は除外する。`toolFormerData` からツール情報を取る。params/result は常に JSON とは限らない。
- bubble の createdAt は欠けていることが多い（実測で138件のみ）。session の時刻を各 bubble に複製しない。
- agentsview の観測: `composerData` の `conversationMap`（inline 形式の候補）は実データで常に空だった（`_v: 16` の composerData、`_v: 3` の bubble）。現行版では外部形式（`fullConversationHeadersOnly` → `bubbleId:*`）を主とする。

**Cursor Agent の transcript**（agentsview。IDE の `state.vscdb` とは別の保存先）
- ルートは `~/.cursor/projects`（上書きは `CURSOR_PROJECTS_DIR`）。メタデータは隣の `~/.cursor/chats`。
- 配置:
  - `<projectDir>/agent-transcripts/<id>.{jsonl,txt}`、または `<projectDir>/agent-transcripts/<id>/<id>.{jsonl,txt}`（トップレベル）
  - `<projectDir>/agent-transcripts/<parentId>/subagents/<childId>.{jsonl,txt}`（subagent）
- **subagent の親はディレクトリ構造で決まる**。親側の transcript には ID が null の Subagent tool_use だけが書かれ、tool_result が無いため、ディレクトリが唯一のリンクになる。
- `<projectDir>` はエンコードされたプロジェクトのパスで、実在するディレクトリを辿って復元する必要がある。

---

## 3. 技術スタック

**採用: Tauri v2（Rust + React / Tailwind CSS + TypeScript）**

選定理由:
1. **低リソース常駐**: 目標はメモリ 30〜50MB 程度（Electron の 1/5 以下）。SQLite と全セッションの状態を保持した状態で実測して確認する。
2. **OS 監視**: Rust の `notify` クレート（Windows `ReadDirectoryChangesW`）による差分検知。
3. **非同期データパイプライン**: SQLite / Protobuf / JSONL のデコードを Tokio のバックグラウンドで実行し、UI の応答性を保つ。

構成要素と制約:
- SQLite: `rusqlite`（bundled）。URI `file:...?mode=ro` で開き、`PRAGMA query_only=ON` を実行する。
- protobuf: AGY の既知 field 番号だけを読む小さな wire デコーダを自作する（スキーマ生成に依存しない）。未知の wire type や長さの不整合は拒否する。
- 監視: `notify` に加えてポーリングと定期照合を併用する（§5.2）。ReadDirectoryChangesW はバッファがあふれるとイベントを落とす。
- ファイルの同一性: `file-id` クレート（Windows の volume serial + file index）。
- Tauri v2 の capability は `core:default` と、イベントの受信に必要な最小限だけにする。shell・fs・opener などのプラグインは MVP では入れない。
- フロントエンドは本文を React のテキストノードとしてだけ描画する。`dangerouslySetInnerHTML` と外部リソースの読み込みは禁止し、CSP で `default-src 'self'` にする。
- **子プロセスの stdin**: Rust の `std::process::Command` は既定で stdin を継承する。子プロセスを起動するときは必ず `.stdin(Stdio::null())` を付ける。
  - Windows では、stdin のパイプを継承した子プロセスの起動が、親のスレッドが同じパイプを読んで待っている間止まる。session recall では、MCP サーバーが stdin を待つ間にバックグラウンドで起動した `git` が止まり、sync が数十分 running のままになった。
  - Tauri の GUI アプリは通常 stdin を持たないが、CLI モード（`--real-logs` の smoke など）や将来の MCP / フック連携では起きる。
- MVP の Project Resolver は `.git` の上方探索で、git を起動しない（§5.5）。

---

## 4. 共通セッションモデル

```typescript
export type Agent = "agy" | "claude_code" | "codex" | "cursor";
export type Completeness = "complete" | "partial" | "unsupported" | "unavailable";
export type StatusValue = "running" | "waiting_input" | "idle" | "error" | "unknown";

export interface UnifiedSession {
  sessionUid: string;            // `${sourceId}:${nativeId}` 。製品間・領域間で衝突しない
  nativeId: string;              // Claude subagent は `${sessionId}::agent:${agentId}`
  agent: Agent;
  sourceId: string;              // claude-local / agy-ide など
  writerVersion?: string;        // ログを書いた版。CLI の版とは別物

  projects: ProjectRef[];        // 所属関係。multi_root なら複数。帰属不明なら空配列
  rawPaths: string[];            // ログ上の cwd / workspace URI（正規化前）

  status: {
    value: StatusValue;
    reason: string;              // 例: "codex:task_started turn=…"、"claude:stop_reason=tool_use Bash"
    confidence: "high" | "low";
    stale: boolean;              // 600秒以上ログの更新が無い（ただし未完了のターンがある）
    autoReview: boolean;         // Codex guardian_review の子が動いている
    since?: string;
  };

  currentTask: {
    title?: string;              // 最初の人間の発言
    lastUserMessage?: string;    // 直近の人間の発言
    lastAssistantMessage?: string;
    activeTool?: { name: string; input: string; callId?: string; startedAt?: string };
  };

  metrics: { stepCount: number; toolCalls: number };

  hierarchy: {
    parentUid?: string;
    childUids: string[];
    kind: "top_level" | "subagent" | "fork" | "automated";   // automated = guardian_review 等
    label?: string;              // Codex agent_role / agent_nickname、Cursor subagentTypeName
  };

  timestamps: {
    startedAt?: string;
    lastActiveAt?: string;
    lastActiveSource: "event" | "file_mtime" | "wal_mtime" | "header";   // wal_mtime: AGY の -wal、header: Cursor の composerHeaders
  };

  archived?: boolean;            // Cursor isArchived

  health: {
    completeness: Completeness;
    diagnostics: { code: string; count: number }[];          // 例: unknown_block, missing_bubble
  };
}

export interface ProjectRef {
  key: string;                   // 比較用に正規化したパス
  name: string;                  // 表示名（ルートディレクトリ名）
  displayPath: string;           // 表示用のパス（大文字小文字は元のまま）
  attribution: "git_root" | "raw_path" | "multi_root" | "alias";
}

export interface SourceHealth {
  sourceId: string;
  agent: Agent;
  rootPath: string;
  state: "ok" | "not_installed" | "empty" | "access_denied" | "unsupported_schema" | "error";
  detail?: string;               // 本文やパスは含めない。エラーの種類だけ
  lastScanAt?: string;
  filesTracked: number;
  sessionsTotal: number;
  partialSessions: number;
  writerVersions: string[];
  topDiagnostics: { code: string; count: number }[];
}
```

- tokens は MVP のモデルに含めない（出所が未検証のため。§10）。将来入れる場合も任意項目にし、0 と「取得できない」を区別する。
- 集計: Project Matrix の件数は `sessionUid` で重複を除いてから数える。multi_root のセッションは複数の行に表示するが、全体の合計には1回だけ数える。

---

## 5. コアコンポーネント設計

### 5.1 Readers

**JSONL Tail Reader（Claude / Codex / AGY transcript）**

- 共有モード `READ | WRITE | DELETE` で開き、読み終わったらすぐに閉じる。ハンドルを持ち続けない。DELETE 共有を付けないと、エージェント側のローテーションや削除・作り直しを妨げる。
- ファイルごとの状態は `(fileId, offset, size, mtime)`。
  - fileId が変わった、またはサイズが offset より小さくなった場合は、置き換えとみなして最初から読み直す（`file_replaced`）。
  - offset は「処理が終わった最後の完全な行の末尾」でだけ進める。改行で終わっていない末尾行はバッファに残し、次の周期で続きを読む（`incomplete_tail`）。
  - 末尾以外の壊れた行は `malformed_record` としてスキップする。前後の正常な行は失わない。
- 起動時はオフセットだけでは状態を復元できないため、先頭（上限付き）＋末尾（上限付き）を読み直して title と最新の状態を復元する。
- 上限（ファイル＝1会話の製品だけに適用する）:

| 項目 | 既定値 | 超えた場合 |
|---|---|---|
| 1行の最大長 | 4 MiB | その行を捨てる。`oversized_line` |
| 未完了の末尾行のバッファ | 4 MiB | バッファを捨てる。次の改行まで読み飛ばす |
| 1回の読み取り量 | 8 MiB | 次の周期で続きを読む |
| 初回の先頭読み取り（ファイルが上限を超える場合） | 512 KiB | — |
| 初回の末尾読み取り（同上） | 8 MiB | 中間を読み飛ばす。`truncated_initial_scan`（partial） |
| JSON のネストの深さ | serde_json の既定（128） | `malformed_record` |

- UTF-8 は完全な1行のバイト列に対して厳格にデコードする。失敗した行は `invalid_utf8` として捨て、状態の確度を low に下げる。置換文字のまま表示したり、CP932 を推定して読み直したりしない。
- Claude の `api-request-blob` のような巨大なレコードがあるため、全行を汎用 JSON 値にせず、`type` だけを先に probe して必要な行だけを parse する。

**SQLite Snapshot Reader（AGY / Cursor、MVP 対象外）**

```
open  file:<path>?mode=ro      (busy_timeout 2s)
PRAGMA query_only=ON
BEGIN                          -- 1回のスナップショット読み取りだけ
  SELECT …                     -- 必要なキーだけ。件数とクエリ時間に上限（progress handler で中断）
COMMIT / close                 -- すぐ閉じる。接続は保持しない
-- デコードはトランザクションを閉じた後に行う
```

- read transaction を保持している間は WAL の checkpoint が進まず、エージェント側の WAL が肥大化する。常時接続やトランザクションの保持はしない。
- ro で開けなかった場合は `unavailable` として表示する。immutable での読み直しや、書き込み可能な接続での再試行はしない。
- 上限は「会話単位」にする:

| 項目 | 既定値 | 超えた場合 |
|---|---|---|
| DB ファイル全体のサイズ | 上限を設けない | — |
| 1会話の値の合計（Cursor: `composerData` + bubble） | 64 MiB | その会話だけ `artifact_too_large`。他の会話は読む |
| 1回の読み取りで処理する会話数 | §5.2 の周期あたりの上限に従う | 次の周期で続きを読む |

- 会話の変更判定は、その会話のキーと値の hash（Cursor）、または会話 DB と `-wal` の (size, mtime)（AGY）で行う。共有ファイル全体の hash は取らない（会話の数だけ 96MB を hash し直すことになる）。

### 5.2 Change Detector と Ingestion Scheduler

- 監視対象は、環境変数と追加 source を反映して解決した後の source 一覧にする。
- 変更検出は3段構えにする。
  1. `notify`（ReadDirectoryChangesW、再帰）。イベントは対象ファイルごとの dirty 集合にまとめる（キューを無制限に積まない）。デバウンスは JSONL で短く（例: 200ms）、頻繁に更新される Cursor の globalStorage では長く（例: 1〜2秒）する。
  2. **ホットファイルのポーリング**: 直近 15 分以内に更新されたファイルの `(size, mtime)` を 2 秒ごとに確認する。書き込み側がファイルを開いたまま追記すると、通知が遅れたり欠けたりするため。
  3. **定期の照合**: 60 秒ごとに source のディレクトリを走査し、新しいファイルと消えたファイルを拾う。notify のバッファがあふれた場合は、すぐに照合する。
- 初回の走査は、最終更新が直近 7 日以内のファイルだけを対象にする（既定値）。それより古いファイルは数だけを SourceHealth に記録する。
- dirty なファイルは、更新が新しい順に処理する。1回の周期で処理する量に上限を設け、巨大なファイル1つが他の更新を止めないようにする。
- source ごとに直列で処理する。同じ DB への読み取りを並行させない。
- 失敗した source は、指数バックオフ（最大 60 秒）で再試行する。
- SQLite の変更は本体ではなく **`-wal` のイベント** を見る。WAL モードでは書き込みがまず `-wal` に入る。
- **AGY**: DB・WAL・transcript の3つを1つの会話の依存ファイルとして扱う。`conversations/` の変更通知と、ホットファイルのポーリングで `-wal` の (size, mtime) を見て、変わった会話だけを再読み込みする。
- **Cursor**: `state.vscdb` / `-wal` の変更通知は「どこかの会話が変わった」合図としてだけ使う。合図が来たら `SELECT composerId, lastUpdatedAt FROM composerHeaders WHERE lastUpdatedAt > :watermark` で変わった会話を特定し、その会話だけを読む。全 bubble の走査は初回だけにする。
  - `lastUpdatedAt` が変わらない更新もあるため（§2.4）、watermark だけに頼らない。最近の会話について、`composerData:<cid>` の値と `bubbleId:<cid>:` 〜 `bubbleId:<cid>;` のキー範囲の値の digest を取り、前回と比べる（agentsview の方式。キーのインデックスに乗るので、その会話の行だけを読む）。

### 5.3 Status Evaluator

セッションごとに「ターン状態機械」を持つ。製品ごとに、ターンの開始・ツールの開始と終了・ターンの終了を、イベントとして抽出する。

**製品ごとのシグナル**

| シグナル | Claude Code | Codex |
|---|---|---|
| ターン開始 | 人の発言 | `task_started` |
| ツール開始 / 終了 | `tool_use` / `tool_result`（id で対応） | `*_call` / `*_call_output`（`call_id` で対応） |
| ターン完了 | assistant の `stop_reason=end_turn` | `task_complete` |
| 中断 | `[Request interrupted by user` | `turn_aborted` |
| エラー | `system/api_error`。`tool_result.is_error` はツールの失敗であり、セッションのエラーではない | `task_complete.error` |

**判定の優先順位**（上から順に評価し、最初に当てはまったものを採る）

| # | 条件 | value | confidence | 備考 |
|---|---|---|---|---|
| 1 | 最新のレコードを読めなかった（未対応形式・UTF-8 不正・truncated の初回走査で末尾が欠けた） | `unknown` | — | |
| 2 | 最後のターンがエラーで終わり、その後の活動が無い | 600 秒以内なら `error`、それ以降は `idle`（reason に error を残す） | high | 古いエラーで一覧が赤く残り続けないようにする |
| 3 | 未完了のターンがある（開始から完了・中断が無い） | `running` | 未解決のツールがある、または 30 秒以内に追記がある: high。どちらでもない: low | 600 秒以上更新が無ければ `stale=true`、confidence は low。**idle にはしない** |
| 4 | 最後のターンが完了・中断で終わった | 600 秒以内なら `waiting_input`、それ以降は `idle` | high | |
| 5 | ターンの境界が見えない（例: 末尾だけを読んだ） | 600 秒以内なら `running`、それ以降は `idle` | low | |

- 600 秒は、session recall の「実行中セッション判定」で使っている閾値（`IDLE_SECONDS`）と同じ。
- Claude では、`stop_reason=tool_use` の後に tool_result が来ても、ターンはまだ完了していない。end_turn まで running。
- Codex の guardian_review（automated）の子が running の間は、親の `autoReview=true`、reason は `auto_review`。親の value は変えない（親はターンの途中なので、すでに running のはず）。
- 子（subagent・fork）が running のとき、親カードに「子が稼働中」を表示する。親の状態には混ぜない。
- 子が running でも、親のターンが子の最後の活動より後に終わっていれば、子は `idle`（confidence low、reason `parent turn ended after last activity`）にする。実ログでは、Claude subagent のファイルの多くが `tool_use` で終わり、終端レコードが書かれていない（2026-10-17 の調査で 80 件中 69 件）。
- `awaiting_approval`（人の承認待ち）は、承認待ちと長時間実行中のコマンドをログでは区別できないため出さない。長時間の無更新は `running (stale)` として表示する。
- AGY（MVP 後）: RUN_COMMAND は exit_code を持つ。それ以外の tool は transcript で補ったものなので、確度を下げる。Cursor の状態は確度 low で扱う。

### 5.4 Session Aggregator

- **階層**: Claude の subagent（`isSidechain` + `agentId`、または `subagents/` 配下のファイル）、Codex の `parent_thread_id` と fork、guardian_review（automated）を `hierarchy.kind` で区別する。kind は Codex では thread_source から決める。一覧の既定は top_level だけを表示し、子は親カードの中に入れ子にする。
- **セッションの切り替えは推定しない**。同じプロジェクトでの並行セッションは普通にあるため、各セッションの状態は §5.3 だけで決める。
- **重複排除**: Codex の event_msg と response_item、AGY の DB と transcript は、正とする stream（response_item、DB）だけを本文に使う。本文が似ているだけの別の発言はまとめない。

**自己観測の除外**

子プロセスに届く環境変数（2026-10-17 実測）:

| 製品 | 会話 ID の変数 | ツールのシェル | MCP サーバー | 備考 |
|---|---|---|---|---|
| Claude Code | `CLAUDE_CODE_SESSION_ID` | 届く | **届く** | 起動時の値のまま。`/clear` 後は古い ID を指す |
| Codex | `CODEX_THREAD_ID` | 届く | 届かない | |
| AGY | `ANTIGRAVITY_CONVERSATION_ID` | 届く | 届かない | `ANTIGRAVITY_TRAJECTORY_ID` は別物 |
| Cursor | 不明 | 未確認 | 届かない（関連する変数なし） | |

- 常駐アプリ（Tauri）はどの製品の子プロセスでもないので、どの変数も届かない。**MVP では自己観測の除外を行わない**。
- 将来、MCP サーバーやフックとして Lens の一部を製品から起動させる場合の判定方法:
  1. Claude Code は環境変数の ID を使う。そのログが 600 秒以上更新されず、同じプロジェクトの別の top_level のログが 600 秒以内に更新されていれば、`/clear` などで切り替わったとみなしてそちらを使う。
  2. 変数が届かない製品は、その製品で、プロジェクト内の top_level のうち直近 600 秒以内に活動した最新の会話を「自分」とみなす。活動時刻は §2.3 / §2.4 の根拠を使う。Codex は guardian を候補から外す（§2.2）。
  3. この判定は並行作業中に別の会話を選ぶことがある。除外した会話を必ず UI に表示する（黙って消さない）。
- 状態の判定（§5.3）にはこの推定を使わない。

### 5.5 Project Resolver

1. `file://` URI をパスに変換し、パーセントデコードと区切り文字の統一を行う。比較用のキーと表示用の文字列を分ける。
   - 比較用のキー: ドライブ文字を大文字にし、区切り文字を `\` にそろえ、末尾の区切り文字を取り除く。大文字小文字は、Windows の既定（区別しない）に合わせて小文字にする。大文字小文字を区別するディレクトリを誤って統合する可能性は残る（既知の制約）。
   - 表示用の文字列: 元の大文字小文字を保つ。`\\?\` の接頭辞は取り除く。
2. `.git` を上方に探索する（`.git` がディレクトリでもファイル＝worktree でも可）。見つかったディレクトリを `git_root` とする。結果はパスごとにキャッシュする。探索の上限は 32 階層とする。UNC パスは探索しない。
   - サブディレクトリの cwd を別プロジェクトとして並べない。
   - `.git` が直下にあるかだけを確認する方式は誤る（サブディレクトリでは見つからない）。
3. 見つからない場合（Git 管理外、cwd が削除・移動済み）は、正規化した cwd を `raw_path` として project にする。
4. multi_root（Cursor の複数 root workspace、AGY の複数 workspace_uris）は、各 root を ProjectRef にし、すべてのプロジェクト行に表示する（MVP 対象外）。
5. cwd がログに無い場合だけ、`projects=[]`（帰属不明）にする。本文中のパスから推測しない。
6. MVP 後: linked worktree は別プロジェクトとして扱う（表示上は同じリポジトリとしてまとめてもよい）。移動前のパスとの対応は、ユーザーが明示した alias だけを使う。

---

## 6. ダッシュボード UI

1. **Agent Cards（最上段）**: エージェントごとに稼働中セッションの数と、最も新しい活動を表示する。
   - 例: `[Claude Code] agent-session-lens: Running Bash 'npm test' (45s) · subagent ×2`
   - 確度が low の状態は `?` を付ける。`stale` が付いていれば `running (stale)`、`autoReview` が付いていれば「自動審査中」バッジを付ける。
2. **Project Matrix（メイン）**: 行に Git ルート（または raw_path）、列にエージェントを並べる。行の末尾に「帰属不明」を常に置く。automated のスレッドは件数に数えない。
3. **Live Inspector（ドリルダウン）**: 直近のメッセージ、ツール履歴（call と result の対応、exit_code、切り詰めの有無）、親子ツリー。MVP では最新の状態だけを表示し、トークン推移などは後回し。
4. **Health**: SourceHealth を source ごとに表示する。
   - `not_installed` と `empty` と `access_denied` を区別する。
   - 最終読み取り時刻、partial になったセッションの数と上位の診断コード、writer version の一覧。
   - ログ形式が変わった場合、まずここに表示が出る。サイレントに 0 件にならないようにする。
- 本文はすべてプレーンテキストで描画する（§3）。

---

## 7. プライバシーとリソースの上限

- 会話本文やツールの引数・出力は、ディスクにキャッシュしない。永続化するのは、オフセット・fingerprint・集計値・診断コードだけにする。
- 表示用の切り詰め（ツール入力 2,000 文字、発言 2,000 文字、出力は先頭と末尾 1,000 文字）と redaction は、Rust 側で snapshot を作るときに適用する。WebView へは処理済みの文字列だけを送る。
- 元ログ側ですでに切り詰められている場合（transcript の `truncated_fields`、RunCommandOutput の truncated）は、それを明示する。
- **既定の redaction**（MVP。ユーザー定義のルールは後回し）:
  - よく知られた秘密情報の形（`sk-…`、`ghp_…`、`AKIA…` など）。
  - 環境変数の代入形式: `(?i)\b[A-Z0-9_]*(TOKEN|SECRET|PASSWORD|API_?KEY|CSRF)[A-Z0-9_]*\s*[=:]\s*\S+` の値の部分を伏せる。実例: ユーザーがエージェントに環境変数の一覧を出させ、その出力を別のエージェントの会話に貼ったため、`ANTIGRAVITY_CSRF_TOKEN=…` が Claude Code のログに残った。
- ツール出力の文字化け: Windows のエラーメッセージ（CP932）が、元ログの時点ですでに U+FFFD に置き換わっていることがある。これは正しい UTF-8 なので `invalid_utf8` にはならない。そのまま表示し、修復しようとしない。
- 診断のログ出力には、件数・診断コード・版番号だけを含める。本文・タイトル・パスは出力しない。
- メモリ上の保持: セッションごとに状態機械と、最新の発言・ツールの文字列（切り詰め済み）だけを持つ。イベント履歴は持たない（MVP）。

---

## 8. テスト方針

- **合成 fixture**: 実際のログをコピーしない。構造だけを再現し、本文・ID・パスは合成値にする。AGY の protobuf は field 番号を保ったまま、値を合成して作る。
- **環境変数の分離**: テストを各エージェントのシェルから実行すると、`CLAUDE_CODE_SESSION_ID` などの環境変数を子プロセスが引き継ぐ。共通の fixture で消す。
- **実機スモーク**: 明示したフラグ（`cargo run --example smoke -- --real-logs`）を付けたときだけ実ログを読む。結果には集計値だけを出力する。
- **回帰テスト**:
  - Reader
    - 不完全な末尾行、ファイルの置き換え（fileId の変更、サイズの縮小）
    - 4KB 未満のファイルへの追記で、置き換えと誤判定しないこと
    - 先頭＋末尾の初回読み取りで、title と最新の状態が取れること
    - UTF-8 として不正なバイト列
    - WAL 付き DB で immutable にフォールバックしないこと
  - 状態判定
    - 優先順位表（§5.3）の各行
    - 長時間実行中のツールが stale になっても idle にならないこと
  - Claude
    - subagent が親と同じ sessionId を持つ場合
    - subagent の別ファイル
  - Codex
    - fork の継承範囲（ordinal）
    - event_msg との二重取り込み
    - guardian_review の autoReview。rollout が title・件数・「最新のセッション」の候補に入らないこと
  - AGY
    - 未知の step_format
    - ファイル名だけで一覧が作れること。`-wal` だけが更新されたときに活動時刻が進むこと
  - Cursor
    - 孤立 composer、NULL の bubble、時刻欠落
    - 無関係な大きい値を含む `state.vscdb` で、上限より小さい会話が読めること。無関係な値が変わっても、その会話を読み直さないこと
    - `composerHeaders.lastUpdatedAt` の更新だけで、その会話が dirty になること
  - 子プロセス: 親が stdin に何も書かない状態で、子プロセスを使う処理が時間内に終わること（Windows で必須。親が定期的に書き込んでいると止まりが解けて再現しない）。

---

## 9. MVP の範囲

| 含める | 後回し |
|---|---|
| Claude Code と Codex の JSONL | AGY（SQLite + protobuf）、Cursor |
| JSONL Tail Reader（上限・fileId・先頭＋末尾の復元） | SQLite Snapshot Reader |
| notify + ホットファイルのポーリング + 60秒照合 | |
| Project Resolver（URI 正規化、`.git` 上方探索、raw_path、帰属不明） | multi_root、alias、linked worktree の統合 |
| Status Evaluator（§5.3 の表） | awaiting_approval（hooks 連携が前提） |
| subagent / fork / automated の階層 | 多世代ツリーの詳細 UI |
| SourceHealth と最小の Session Health | writer version ごとの詳細 |
| Rust 側での切り詰めと既定の redaction | ユーザー定義の redaction、tokens、トークン推移 |
| Agent Cards、Project Matrix、Inspector（最新の状態だけ）、Health | トレイ、トースト、単一インスタンス |

Cursor と AGY は後回しだが、§2.3 / §2.4 / §5.1 / §5.2 で読み取りの方式は確定している。MVP 後に着手するときは、ここから始める。

---

## 10. 未検証・初期対象外

- **トークン値の出所**: agentsview の実装で形式が分かった（下表）。本プロジェクトの実ログでは未確認なので、MVP では UI に出さない。AGY の usage 系テーブルは使用量であり、本文の代わりにはならない。

| 製品 | 出所 | フィールド | 注意 |
|---|---|---|---|
| Claude | assistant の `message.usage` | `input_tokens`（キャッシュ外）、`cache_creation_input_tokens`、`cache_read_input_tokens`、`output_tokens` | 同じ `message.id` の分割レコードは最後の1件だけを数える（§2.1） |
| Codex | `event_msg` の `token_count` の `info.last_token_usage` | `input_tokens`（**キャッシュ分を含む**）、`cached_input_tokens`、`output_tokens`、`cache_write_input_tokens`（あれば） | キャッシュ外 = input − cached − cache_write。直前の assistant 発言に紐づける |
| AGY | SQLite の `gen_metadata` テーブル（`idx, data`）。data は protobuf | field 1（chat_model）→ field 4（usage = ModelUsageStats）→ 1 model、2 input（キャッシュ外）、3 output（thinking を含む）、4 cache write、5 cache read（0 なら省略） | agentsview は sidecar の `generatorMetadata[].chatModel.usage` と 550/550 件で一致を確認。reasoning の内訳は無い |

- **承認待ちの判定**: ログだけでは確定できないため MVP では出さない。agentsview も、未解決の tool call を「承認待ち・実行中・クラッシュ」のどれか区別できないとして `tool_call_pending` にまとめている。各製品の hooks（AGY の Hooks は conversationId / workspacePaths を渡す）を補助に使う案は、将来の検討事項とする。
- **Cursor**
  - メインで使う PC での `state.vscdb` の大きさと読み取り時間。調査した PC は無料プランで会話が少なく、composer 単位の読み取りで 161 件を判定するのに初回約 10 秒、2 回目以降（変更なし）約 2〜4 秒だった（Python 実装での値）。agentsview のコメントには「数百 MB」とあり、大きくなる前提は正しい。
  - IDE の subagent の親 composer ID。agentsview も IDE 側（`state.vscdb`）では親子を取っていない。Cursor Agent の transcript ではディレクトリ構造で親が決まる（§2.4）。
  - MCP サーバー・ツールのシェルに渡す環境変数（エージェントの利用上限に達して確認できなかった。agentsview にも記述なし）。
- **Codex**: guardian・`codex_exec` 以外の automated スレッドの種類。agentsview は `thread_source` の値として `roborev`（agentsview 自身のレビュー機能）も扱っており、外部ツールが独自の値を書くことがある。未知の値は automated ではなく top_level として表示し、SourceHealth の診断に値を出す。
- **Claude**: 新しい版で subagent の配置がさらに変わる可能性がある。未知の配置は SourceHealth の診断に出す。
- **対象外**
  - WSL 内のログ（`\\wsl$`）。通知が効かないため、将来はポーリング専用の source として扱う。Windows と WSL をまたいで同じ稼働中の DB を読むことも対象外。
  - ネットワーク上の WAL DB、Cursor の Cloud Agent のリモート履歴、各製品の公式 export 形式。
- **確認範囲**: 製品のすべての版での互換性は確認していない。確認したのは調査時点の版（例: Codex writer 0.162.0-alpha、Cursor 3.20.10）だけ。未知の形式は §6 の Health に表示する。
