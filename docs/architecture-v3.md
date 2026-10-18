# Agent Session Lens アーキテクチャ設計書 v3

v2（[architecture-v2.md](architecture-v2.md)）を、合議レビュー（[docs/gougi/2026-10-17-architecture-v1-v2-review.md](gougi/2026-10-17-architecture-v1-v2-review.md)）と、2026-10-17 に実ログの構造（キーと型だけ）を再調査した結果で改訂したもの。
v2 の内容のうち、ここで変更していない部分（§2.3 AGY、§2.4 Cursor の形式仕様など）は v2 を参照する。

## 0. v2からの主な変更点

| # | v2 | v3 | 理由 |
|---|---|---|---|
| 1 | ro で開けなければ immutable で読み直す（WAL/journal不在＋fingerprint不変が条件） | **削除**。ro で読めなければ `unavailable` | WAL モードのDBは最後の接続が閉じると `-wal` が消える。fingerprint の比較では読み取り中の変更を検出できない。immutable は呼び出し側が不変を保証する契約で、稼働中のDBには使えない |
| 2 | 状態は「最後のイベントの種類」で判定。guardian_review の間は親を `awaiting_approval`(high) にする | 終端シグナルを基準にする。状態の優先順位を表で決める。600秒の無更新は `stale` 属性にする。guardian_review は `running` + `auto_review` | 未解決の call は中断やクラッシュでも残る。テキスト応答は途中経過でも出る。guardian_review は自動審査で、人の承認待ちではない（§2.2 と矛盾していた） |
| 3 | 永続化するのはオフセットだけ。64MiB超は末尾だけ読む | 起動時は **先頭（上限付き）＋末尾（上限付き）** を読み直して状態を復元する。置換は **ファイルID** で検知する | オフセットだけでは summary や未解決 call を復元できない。先頭 4KB の hash は、4KB 未満のファイルでは通常の追記でも変わる |
| 4 | Health はセッション単位だけ | **SourceHealth** を新設する | 一度も読めない source には UnifiedSession が存在せず、0件と区別できない |
| 5 | 同じプロジェクトで新しいセッションが動いたら古い方を idle にする | **削除** | 同じプロジェクトでの並行セッションは普通にある。古いセッションが置き換えられた証拠にならない |
| 6 | redaction は表示の直前に適用。上限は表示時の切り詰めだけ | 切り詰めと redaction を **Rust 側で IPC 送信前** に行う。読み取り段階にも上限を置く。Tauri の権限は最小にする | WebView に生の本文を渡さないため。巨大な行は表示前の段階でメモリを食う |
| 7 | Codex fork の継承範囲は「timestamp が開始より前」 | `subagent_history_start_ordinal`（行位置）で判定する | 実ログでは、継承されたレコードの timestamp が fork 時刻に書き換わっている。timestamp では判定できない |
| 8 | Claude subagent は同じファイル内の sidechain | `<sessionId>/subagents/agent-<agentId>.jsonl` という別ファイルも扱う | 実ログで確認した |
| 9 | `project.key` は1つ | セッションと project の所属関係を `projects[]` で持つ。`raw_path` の帰属を追加する | multi_root を表現するため。Git 管理外の cwd を帰属不明にしないため |
| 10 | summary は最初の人間の発言 | `title`（最初の依頼）と `lastUserMessage`（直近の依頼）を分ける | 再開したセッションでは、最初の依頼が現在の作業と一致しない |

v2 の文書上の誤り: §9 の「§6.4」は §6 の 4. Health を指す。§0 #2 の理由欄は WAL の肥大化（read transaction の保持）の話で、`-shm` の話は sandbox 環境での別の事象。

---

## 1. 概要と設計原則

v2 §1 を維持する。原則1「元ログに書き込まない」の範囲は次のとおり明確にする。

- 会話データの論理的な内容を変更しない。書き込み可能な接続やファイルハンドルを開かない。
- SQLite の ro 接続は、WAL の読み取りのために `-shm` を共有メモリとして扱うことがある。これは SQLite の仕様の範囲として許容する。`-shm` を開けなければ `unavailable` にする。

---

## 2. エージェント別仕様（v2 からの差分）

### 2.1 Claude Code（jsonl-v2）

- ファイル配置:
  - `projects/<encoded-cwd>/<sessionId>.jsonl`（トップレベル）
  - `projects/<encoded-cwd>/<sessionId>/subagents/agent-<agentId>.jsonl`（subagent）。同じディレクトリの `*.meta.json` は読まない。
- 人の発言は次のように判定する。
  - `type=user`、`isMeta` が無い、tool_result を含まない、という3つを満たす。
  - text block ごとに、先頭の自動挿入タグ（`<ide_opened_file>`、`<ide_selection>`、`<system-reminder>`、`<browser_instruction>`、`<local-command-*>`、`<task-notification>` など）を取り除く。何も残らない block は使わない。
  - `<command-name>X</command-name>` と `<command-args>Y</command-args>` は、`X Y` という人の発言として扱う。
  - `[Request interrupted by user…` は中断のシグナルとして扱う（§5.3）。
- 終端シグナル: assistant レコードの `message.stop_reason`。`end_turn` はターンの完了、`tool_use` はツールの実行待ちを表す。
- エラー: `type=system, subtype=api_error`。

### 2.2 Codex CLI（rollout-v1）

- ターンのライフサイクル（`event_msg`、実ログで確認）:
  - `task_started`: ターンの開始。
  - `task_complete`: ターンの完了。`error` を含む場合がある。
  - `turn_aborted`: ターンの中断。`reason` を含む。
- assistant message の `phase`: `commentary`（途中経過）、`final_answer`（最終回答）。
- 発言の本文は `response_item` を正とする。`event_msg` は状態の判定にだけ使い、本文としては取り込まない。
- 先頭の `session_meta` が自分のメタデータ。2つ目以降の `session_meta` は fork 元のもの。
  - 親は `parent_thread_id`（または `source.subagent.thread_spawn.parent_thread_id`）で決める。
  - 継承範囲: 行番号が `subagent_history_start_ordinal` より小さいレコードは fork 元から継承したもので、このセッションの活動に数えない。ordinal が無いのに2つ目の session_meta がある場合は、継承範囲を推定しない。継承されたレコードには `task_started` も含まれるため、レコードの種類では境界を決められない。この場合は全体を取り込み、`fork_boundary_unknown` の診断を付けて状態の確度を low にする。
- `thread_source`: `user`（top_level）、`subagent`、`guardian_review`（automated）。
- user 発言のうち、`<` で始まる block と `# AGENTS.md instructions` で始まる block は自動挿入として除外する。

### 2.3 AGY / 2.4 Cursor

v2 から変更なし（MVP の対象外）。§5.1 の SQLite Reader の変更だけを適用する。

---

## 3. 技術スタック

v2 を維持する。追加は次のとおり。

- ファイルの同一性は `file-id` クレートで取る（Windows の volume serial + file index）。
- Tauri v2 の capability は `core:default` と、イベントの受信に必要な最小限だけにする。shell・fs・opener などのプラグインは MVP では入れない。
- フロントエンドは本文を React のテキストノードとしてだけ描画する。`dangerouslySetInnerHTML` と外部リソースの読み込みは禁止し、CSP で `default-src 'self'` にする。

---

## 4. 共通セッションモデル（v3）

```typescript
export type Agent = "agy" | "claude_code" | "codex" | "cursor";
export type Completeness = "complete" | "partial" | "unsupported" | "unavailable";
export type StatusValue = "running" | "waiting_input" | "idle" | "error" | "unknown";

export interface UnifiedSession {
  sessionUid: string;            // `${sourceId}:${nativeId}`
  nativeId: string;
  agent: Agent;
  sourceId: string;
  writerVersion?: string;

  projects: ProjectRef[];        // 所属関係。multi_root なら複数。帰属不明なら空配列
  rawPaths: string[];

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
    kind: "top_level" | "subagent" | "fork" | "automated";
  };

  timestamps: {
    startedAt?: string;
    lastActiveAt?: string;
    lastActiveSource: "event" | "file_mtime";
  };

  health: {
    completeness: Completeness;
    diagnostics: { code: string; count: number }[];
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

集計: Project Matrix の件数は `sessionUid` で重複を除いてから数える。multi_root のセッションは複数の行に表示するが、全体の合計には1回だけ数える。

---

## 5. コアコンポーネント設計

### 5.1 Readers

**JSONL Tail Reader**

- 共有モード `READ | WRITE | DELETE` で開き、読み終わったらすぐに閉じる。ハンドルを持ち続けない（エージェントがファイルを作り直すのを妨げないため）。
- ファイルごとの状態は `(fileId, offset, size, mtime)`。
  - fileId が変わった、またはサイズが offset より小さくなった場合は、置き換えとみなして最初から読み直す（`file_replaced`）。
  - offset は「処理が終わった最後の完全な行の末尾」でだけ進める。
- 上限:

| 項目 | 既定値 | 超えた場合 |
|---|---|---|
| 1行の最大長 | 4 MiB | その行を捨てる。`oversized_line` |
| 未完了の末尾行のバッファ | 4 MiB | バッファを捨てる。次の改行まで読み飛ばす |
| 1回の読み取り量 | 8 MiB | 次の周期で続きを読む |
| 初回の先頭読み取り（ファイルが上限を超える場合） | 512 KiB | — |
| 初回の末尾読み取り（同上） | 8 MiB | 中間を読み飛ばす。`truncated_initial_scan`（partial） |
| JSON のネストの深さ | serde_json の既定（128） | `malformed_record` |

- UTF-8 は完全な1行のバイト列に対して厳格にデコードする。失敗した行は `invalid_utf8` として捨てる。その行の情報が失われるので、状態の確度を low に下げる。

**SQLite Snapshot Reader（MVP 対象外）**

v2 §5.1 の手順から immutable のフォールバックを除いたもの。`busy_timeout` 2秒に加え、クエリの実行時間に上限（progress handler で中断）を設ける。取得件数にも上限を設ける。デコードはトランザクションを閉じた後に行う。

### 5.2 Change Detector と Ingestion Scheduler

- 監視対象は、環境変数と追加 source を反映して解決した後の source 一覧にする。
- 変更検出は3段構えにする。
  1. `notify`（ReadDirectoryChangesW、再帰）。イベントは対象ファイルごとの dirty 集合にまとめる（キューを無制限に積まない）。
  2. **ホットファイルのポーリング**: 直近 15 分以内に更新されたファイルの `(size, mtime)` を 2 秒ごとに確認する。書き込み側がファイルを開いたまま追記すると、通知が遅れたり欠けたりするため。
  3. **定期の照合**: 60 秒ごとに source のディレクトリを走査し、新しいファイルと消えたファイルを拾う。notify のバッファがあふれた場合は、すぐに照合する。
- 初回の走査は、最終更新が直近 7 日以内のファイルだけを対象にする（既定値）。それより古いファイルは数だけを SourceHealth に記録する。
- dirty なファイルは、更新が新しい順に処理する。1回の周期で処理する量に上限を設け、巨大なファイル1つが他の更新を止めないようにする。
- 失敗した source は、指数バックオフ（最大 60 秒）で再試行する。

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

- Claude では、`stop_reason=tool_use` の後に tool_result が来ても、ターンはまだ完了していない。end_turn まで running。
- Codex の guardian_review（automated）の子が running の間は、親の `autoReview=true`、reason は `auto_review`。親の value は変えない（親はターンの途中なので、すでに running のはず）。
- 子（subagent・fork）が running のとき、親カードに「子が稼働中」を表示する。親の状態には混ぜない。
- 子が running でも、親のターンが子の最後の活動より後に終わっていれば、子は `idle`（confidence low、reason `parent turn ended after last activity`）にする。実ログでは、Claude subagent のファイルの多くが `tool_use` で終わり、終端レコードが書かれていない（2026-10-17 の調査で 80 件中 69 件）。

### 5.4 Session Aggregator

- 階層: Claude の subagent（`isSidechain` + `agentId`、または `subagents/` 配下のファイル）、Codex の `parent_thread_id`。kind は thread_source から決める。
- **セッションの切り替えは推定しない**（v2 の規則を削除）。各セッションの状態は §5.3 だけで決める。
- 重複排除: Codex は本文に response_item だけを使う。event_msg は状態の判定だけに使う。
- 自己観測の除外: MVP では行わない。

### 5.5 Project Resolver

1. `file://` URI をパスに変換し、パーセントデコードと区切り文字の統一を行う。比較用のキーと表示用の文字列を分ける。
   - 比較用のキー: ドライブ文字を大文字にし、区切り文字を `\` にそろえ、末尾の区切り文字を取り除く。大文字小文字は、Windows の既定（区別しない）に合わせて小文字にする。大文字小文字を区別するディレクトリを誤って統合する可能性は残る（既知の制約）。
   - 表示用の文字列: 元の大文字小文字を保つ。`\\?\` の接頭辞は取り除く。
2. `.git` を上方に探索する（`.git` がディレクトリでもファイル＝worktree でも可）。見つかったディレクトリを `git_root` とする。結果はキャッシュする。探索の上限は 32 階層とする。UNC パスは探索しない。
3. 見つからない場合（Git 管理外、cwd が削除・移動済み）は、正規化した cwd を `raw_path` として project にする。
4. multi_root（Cursor・AGY）は、各 root を ProjectRef にする（MVP 対象外）。
5. cwd がログに無い場合だけ、`projects=[]`（帰属不明）にする。本文中のパスから推測しない。

---

## 6. ダッシュボード UI

v2 §6 を維持し、次を変更する。

- Agent Cards: 状態に `stale` が付いていれば `running (stale)` と表示する。`autoReview` が付いていれば「自動審査中」バッジを付ける。low の状態には `?` を付ける。
- Health パネルは SourceHealth を表示する。`not_installed` と `empty` と `access_denied` を区別して表示する。
- 本文はすべてプレーンテキストで描画する（§3）。

---

## 7. プライバシーとリソースの上限

v2 §7 を維持し、次を変更する。

- 表示用の切り詰め（ツール入力 2,000 文字、発言 2,000 文字）と redaction は、Rust 側で snapshot を作るときに適用する。WebView へは処理済みの文字列だけを送る。
- MVP の redaction は、既定のルール（`sk-…`、`ghp_…`、`AKIA…` など、よく知られた秘密情報の形）だけを持つ。ユーザー定義のルールは後回し。
- メモリ上の保持: セッションごとに状態機械と、最新の発言・ツールの文字列（切り詰め済み）だけを持つ。イベント履歴は持たない（MVP）。

---

## 8. テスト方針

v2 §8 に以下を追加する。

- 状態の優先順位表（§5.3）の各行に対応するテスト。
- 長時間実行中のツールが stale になっても idle にならないこと。
- Codex fork の継承範囲（ordinal）と guardian_review の autoReview。
- Claude subagent の別ファイル。
- 置き換えの検知（fileId の変更、サイズの縮小）。
- 先頭＋末尾の初回読み取りで、title と最新の状態が取れること。
- 4KB 未満のファイルへの追記で、置き換えと誤判定しないこと。

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

## 10. 未検証・初期対象外

v2 §9 に以下を追加する。

- 承認待ち（`awaiting_approval`）は、ログだけでは判定できないため MVP では出さない。長時間の無更新は `running (stale)` として表示する。
- WSL 内のログ（`\\wsl$`）は対象外とする。通知が効かないため、将来はポーリング専用の source として扱う。
- Claude の新しい版で subagent の配置がさらに変わる可能性がある。未知の配置は SourceHealth の診断に出す。
