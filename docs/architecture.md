# Agent Session Lens アーキテクチャ設計書

## 1. 概要
AGY (Antigravity), Claude Code, Codex CLI, Cursor の4つのAIエージェントのローカルセッションファイルを直接監視（ReadOnly / 非破壊）し、各エージェントがどのプロジェクトでどのタスクを実行しているかをリアルタイムに一覧化・集約する Windows デスクトップダッシュボード。

```mermaid
flowchart TD
    subgraph Storage [エージェントローカルストレージ]
        A1["AGY (.gemini/.../conversations/*.db + transcript.jsonl)"]
        A2["Claude Code (~/.claude/projects/**/*.jsonl)"]
        A3["Codex (~/.codex/sessions/**/*.jsonl)"]
        A4["Cursor (%APPDATA%/Cursor/.../state.vscdb)"]
    end

    subgraph Core [Windows アプリ Core (Tauri v2 / Rust)]
        W["FileWatcher (ReadDirectoryChangesW + Debounce)"]
        L["Lock-free Ingestion Engine (mode=ro, tail-read)"]
        
        subgraph Adapters [Session Adapters]
            AD1["AGY Adapter (Protobuf Wire + Steps)"]
            AD2["Claude Adapter (JSONL Tool & Msg Parser)"]
            AD3["Codex Adapter (Rollout Event Stream)"]
            AD4["Cursor Adapter (cursorDiskKV Composer Parser)"]
        end

        NORM["Session Aggregator & State Machine"]
    end

    subgraph UI [Dashboard Frontend (WebView2 / React)]
        D1["Agent Activity Matrix (何が・どこで・何を作業中か)"]
        D2["Active Task & Command Stream"]
        D3["Token / Step Metrics"]
    end

    Storage -->|File Events| W
    W --> L
    L --> Adapters
    Adapters --> NORM
    NORM -->|State Diff / IPC| UI
```

---

## 2. エージェント別ストレージ仕様 & 取得方式

| エージェント | セッション保存パス (Windows) | データ形式 | 読み取り方式・排他対策 | 取得可能な主要データ |
|---|---|---|---|---|
| **Claude Code** | `%USERPROFILE%\.claude\projects\**\*.jsonl` | JSONL | ファイル末尾差分追跡 (Tail)。未完了行のバッファリング | `cwd` (プロジェクトパス), 最新 `user` 発話, `tool_use` (Bashコマンド/ファイル編集) |
| **Codex CLI** | `%USERPROFILE%\.codex\sessions\**\rollout-*.jsonl` | JSONL | ファイル末尾差分追跡。先頭 `session_meta` から thread_id 取得 | `cwd`, `response_item` (プロンプト/指示), `custom_tool_call`, サブエージェント親子関係 |
| **AGY (Antigravity)** | `%USERPROFILE%\.gemini\{antigravity-ide,antigravity-cli}\conversations\*.db`<br>`+ brain\<id>\...\transcript.jsonl` | SQLite + Protobuf + JSONL | URI `file:<path>?mode=ro&immutable=1` 接続。書き込みロック回避必須 | `trajectory_metadata_blob` (cwd), `steps` (step_type 14:発話, 15:応答, 21:コマンド実行) |
| **Cursor** | `%APPDATA%\Cursor\User\globalStorage\state.vscdb`<br>`%APPDATA%\Cursor\User\workspaceStorage\*\state.vscdb` | SQLite (WAL) | `mode=ro`。`composerHeaders` と `cursorDiskKV` をクエリ | `workspace.json` (cwd), `composerData:<id>` (会話タイトル), `bubbleId:<id>:*` (プロンプト/ツール実行) |

---

## 3. アプリケーション技術スタック選定

**採用: Tauri v2 (Rust + React / Tailwind CSS + TypeScript)**

- **選定理由**:
  1. **低リソース常駐**: メモリ消費 30〜50MB 程度（Electron の 1/5 以下）。
  2. **堅牢な OS 監視**: Rust の `notify` クレート（Windows `ReadDirectoryChangesW` 最適化）による高精度・低負荷な差分検知。
  3. **非同期データパイプライン**: SQLite / Protobuf / JSONL のデコード処理を Tokio スレッドプールで安全にバックグラウンド実行し、UI の応答性を完全に担保。

---

## 4. 共通セッションモデル (Unified Data Model)

```typescript
export interface UnifiedSession {
  sessionId: string;
  agent: "agy" | "claude_code" | "codex" | "cursor";
  projectName: string;
  projectPath: string;
  status: "running" | "waiting_input" | "idle" | "error";
  currentTask: {
    summary: string;             // ユーザーの初期プロンプト / 直近のゴール
    lastUserMessage: string;     // 直近のユーザー発話
    activeToolName?: string;     // 実行中ツール (bash, edit, read_file 等)
    activeToolInput?: string;    // コマンド内容や対象ファイルパス
  };
  metrics: {
    cachedTokens: number;
    uncachedTokens: number;
    outputTokens: number;
    totalBilledTokens: number;
    stepCount: number;
  };
  hierarchy: {
    isSubagent: boolean;
    parentId?: string;
    subagentIds: string[];
  };
  timestamps: {
    startedAt: string;
    lastActiveAt: string;
  };
}
```

---

## 5. コアコンポーネント設計 & 実装方針

### (1) Lock-free Ingestion Engine (排他制御対策)
- **JSONL (Claude Code / Codex)**:
  - 共有リードオープン (`FileShare.ReadWrite`)。
  - セッションごとの読み取りバイトオフセットを保持し、末尾の差分行のみを読み取る。
  - 行末が改行で終わっていない場合は次回のイベントまでバッファに保持（追記中の不完全行のパースエラー防止）。
- **SQLite (AGY / Cursor)**:
  - 接続文字列に `?mode=ro&immutable=1` を指定。
  - `PRAGMA query_only = ON;` を発行。
  - WAL ファイル存在時もライタープロセスを一切ブロックせず、最新コミット済みスナップショットを取得。

### (2) State Aggregator & Status Evaluator (状態判定ロジック)
最新のイベント種別からセッション状態をリアルタイム判定：
- **`running`**: 未解決のツール呼び出し（Tool Call 発行後、Tool Result 未受信）、またはアシスタントのストリーミング中。
- **`waiting_input`**: アシスタントの最終回答が完了し、ユーザーからの次プロンプトを待機している状態。
- **`idle`**: 最終アクティビティから一定時間（例: 10分以上）イベントが発生していない状態。

---

## 6. ダッシュボード UI 構成案

1. **Active Agent Cards (最上段)**:
   - 4エージェントごとに現在進行中のタスクをカード形式でハイライト表示。
   - 例: `[Claude Code] agent-session-lens: Running 'npm test' (45s elapsed)`
2. **Project Matrix (メイン)**:
   - ワークスペース / リポジトリごとにグルーピング。
   - 各プロジェクトでどのアシスタントが稼働中かをマトリクス表示（複数エージェント協調・並行作業の可視化）。
3. **Live Inspector (サイド / ドリルダウン)**:
   - 選択セッションの直近のログ、実行ツール履歴、トークン消費推移、サブエージェント親子ツリー。
   - ディレクトリを開くショートカット連携。
