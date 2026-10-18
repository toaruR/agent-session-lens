# Agent Session Lens アーキテクチャ設計書 v4

v3（[architecture-v3.md](architecture-v3.md)）を、mcp-server-bm25-code-search のセッション検索（session recall）を4製品の実ログで動かしたときの知見（2026-10-17）で改訂したもの。
v3 の内容のうち、ここで変更していない部分は v3 を参照する。v3 が参照している v2 の部分も同様。

知見の出典: mcp-server-bm25-code-search のコミット `7418591`、`ce7d6d1`、同リポジトリの `docs/session-recall.md`、`docs/research/session-recall/usefulness-eval-2026-10-17.md`。

## 0. v3からの主な変更点

| # | v3 | v4 | 理由 |
|---|---|---|---|
| 1 | （v2 §5.4）`CLAUDE_CODE_SESSION_ID` / `CODEX_THREAD_ID` が届けば自己観測を除外できる | 子プロセスに届く環境変数を製品ごとに表で確定する（§5.4）。**MCP サーバーに ID が届くのは Claude Code だけ** | 2026-10-17 に各製品の MCP プロセスの環境を実測した。Codex と AGY はシェルのツールには ID を渡すが、MCP サーバーには渡さない |
| 2 | Cursor の変更検出・上限はファイル単位（v3 §5.1 の上限表） | Cursor は **composer 単位**で変更を検出し、上限も composer 単位にする（§5.1、§5.2） | 全プロジェクトの全会話が1つの `state.vscdb` に入る。軽く使っているだけの PC でも 96MB あり、ファイル単位の上限では全会話が読めなくなった |
| 3 | Cursor の lastActiveAt は bubble の createdAt、無ければ composerHeaders | 状態の判定（直近の活動）には `composerHeaders.lastUpdatedAt` を使う（§2.4） | `state.vscdb` の mtime はどの会話が更新されても変わるので、会話ごとの活動の根拠にならない |
| 4 | AGY の活動時刻の根拠は未定義 | `conversations/<id>.db` と `-wal` の mtime の新しい方。`presence/*.lock` は使わない（§2.3） | lock は 0 バイトで、終わった会話の分も大量に残っている |
| 5 | AGY のセッション ID は `cascade_id`、無ければファイル名 | ファイル名だけで ID が決まる（DB を開かずに一覧できる） | 実ログ 504 件で `cascade_id` とファイル名が全件一致した |
| 6 | Codex guardian_review は親に `autoReview` を付ける | それに加え、guardian の本文を title・lastUserMessage・件数の根拠に使わない。「最新のセッション」を選ぶ処理から除外する（§2.2） | guardian の user 発言は親の会話の写しで、親と同じ秒に作られる。最新のファイルを選ぶと guardian を誤って選ぶ |
| 7 | 子プロセスの扱いは未定義 | 子プロセスを起動するときは、必ず stdin を null にする（§3） | Windows では、stdin のパイプを継承した子プロセスの起動が、親が stdin を読んでいる間止まる。session recall で sync が数十分止まった |
| 8 | redaction は既定ルール（`sk-…` など）だけ | 環境変数の代入形式（`*_TOKEN=…` など）を既定ルールに加える（§7） | ユーザーが環境変数の一覧を会話に貼り、ローカルのトークンがログに残った実例がある |
| 9 | Claude の自動挿入タグの一覧 | `<command-message>` を追加する（§2.1） | セッションのタイトルが `<command-message>smart-commit</command-message>` になる実例がある |

---

## 2. エージェント別仕様（v3 からの差分）

### 2.1 Claude Code

- 自動挿入タグに `<command-message>` を加える。`<command-name>` / `<command-args>` を人の発言にする規則（v3）は維持する。
- `/clear` で新しい JSONL ファイルが作られ、新しい sessionId になる。プロセスの環境変数 `CLAUDE_CODE_SESSION_ID` は**起動時の値のまま**変わらない（§5.4 で使う場合の注意）。

### 2.2 Codex CLI

- 保存場所: `sessions/YYYY/MM/DD/rollout-<時刻>-<id>.jsonl`。先頭の `session_meta` に `id`、`cwd`、`parent_thread_id`、`thread_source` がある。
- 表示に使える補助情報（先頭の `session_meta`）:

| フィールド | 例 | 用途 |
|---|---|---|
| `originator` | `Codex Desktop` | クライアントの種類 |
| `thread_source` | `user` / `subagent` / `guardian_review` | `hierarchy.kind` |
| `source.subagent` | `{"other": "guardian"}`、`{"thread_spawn": {...}}` | guardian の判定の補助 |
| `agent_role`、`agent_nickname`、`agent_path` | `sg-verifier`、`Russell`、`/root/.../verifier_r1` | subagent のラベル |
| `forked_from_id` | | fork 元 |

- **guardian_review の扱い**（v3 の autoReview に追加）:
  - user 発言は「The following is the Codex agent history whose request action you are assessing…」で始まる、親の会話の写しになっている。title・lastUserMessage・lastAssistantMessage・metrics に使わない。
  - 親の作業中に、親と同じ秒で次々に作られる。「プロジェクトで最新のセッション」を選ぶ処理（§5.4 のフォールバック）では、`thread_source=user` の top_level だけを候補にする。
  - Project Matrix の件数に数えない。

### 2.3 AGY

v2 §2.3 の形式仕様を維持し、次を追加する。

- セッション ID: `conversations/<id>.db` のファイル名。`trajectory_meta.cascade_id` と全件一致した（2026-10-17、504件）。一覧はディレクトリの走査だけで作れる。中身を読むのは変更があった会話だけにする。
- 活動時刻: `mtime(<id>.db)` と `mtime(<id>.db-wal)` の新しい方。WAL モードでは書き込みがまず `-wal` に入るため。
- `presence/<id>.lock` は使わない。0 バイトで、終わった会話の分も残り続ける（cli で 341 件）。
- 実行中の会話 ID は、ツールが実行するシェルには `ANTIGRAVITY_CONVERSATION_ID` として渡される。`ANTIGRAVITY_TRAJECTORY_ID` は別の値で、会話 ID ではない。MCP サーバーには渡されない（§5.4）。
- project の帰属: `trajectory_metadata_blob` に workspace URI が無い会話が多い。実測では cli の 349 件中 203 件が帰属不明だった。帰属不明として表示し、推測で割り当てない（v3 §5.5 の 5 を維持）。

### 2.4 Cursor

v2 §2.4 の形式仕様を維持し、次を追加する。

- 全プロジェクトの全会話が `globalStorage/state.vscdb` 1つに入る。軽く使っている PC でも 96MB あった。メインで使う PC ではさらに大きいと想定する。
- `composerHeaders` の列（実測）: `composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, checkpointAt, value, subagentTypeName`。
  - `lastUpdatedAt`（ミリ秒）を会話ごとの活動時刻にする。`state.vscdb` の mtime は使わない。
  - `isSubagent` / `subagentTypeName` で `hierarchy.kind=subagent` を表せる（ただし親の composer ID の列は未確認）。
  - `isArchived=1` は既定の一覧から外す。
- 会話1件の中身は、`composerData:<cid>` と `bubbleId:<cid>:*` の値。サイズの上限と変更の判定はこの範囲で行う（§5.1）。
- project の帰属の実測: 161 件中、他プロジェクト 102、帰属不明 58（workspace が削除済みなど）、未対応形式 1。帰属不明が多い前提で UI を作る。

---

## 3. 技術スタック（追加）

- **子プロセスの stdin**: Rust の `std::process::Command` は既定で stdin を継承する。子プロセスを起動するときは必ず `.stdin(Stdio::null())` を付ける。
  - Windows では、stdin のパイプを継承した子プロセスの起動が、親のスレッドが同じパイプを読んで待っている間止まる。session recall では、MCP サーバーが stdin を待つ間にバックグラウンドで起動した `git` が止まり、sync が数十分 running のままになった。
  - Tauri の GUI アプリは通常 stdin を持たないが、CLI モード（`--real-logs` の smoke など）や将来の MCP / フック連携では起きる。テストで親が定期的に書き込んでいると止まりが解けて再現しないので、**親が何も書かない状態**でテストする。
- MVP の Project Resolver は `.git` の上方探索（v3 §5.5）で、git を起動しない。この方針を維持する。

---

## 4. 共通セッションモデル（v4 の差分）

```typescript
export interface UnifiedSession {
  // v3 のフィールドはすべて維持する
  hierarchy: {
    parentUid?: string;
    childUids: string[];
    kind: "top_level" | "subagent" | "fork" | "automated";
    label?: string;              // 追加。Codex agent_role / agent_nickname、Cursor subagentTypeName
  };
  timestamps: {
    startedAt?: string;
    lastActiveAt?: string;
    lastActiveSource: "event" | "file_mtime" | "wal_mtime" | "header";   // 追加: AGY の -wal、Cursor の composerHeaders
  };
  archived?: boolean;            // 追加。Cursor isArchived
}
```

---

## 5. コアコンポーネント設計（v3 からの差分）

### 5.1 Readers

**SQLite Snapshot Reader の上限は「会話単位」にする**

| 項目 | 既定値 | 超えた場合 |
|---|---|---|
| DB ファイル全体のサイズ | 上限を設けない | — |
| 1会話の値の合計（Cursor: `composerData` + bubble） | 64 MiB | その会話だけ `artifact_too_large`。他の会話は読む |
| 1回の読み取りで処理する会話数 | v3 §5.2 の周期あたりの上限に従う | 次の周期で続きを読む |

- v3 の JSONL の上限表は、ファイル＝1会話の製品（Claude / Codex / AGY transcript）だけに適用する。
- 会話の変更判定は、その会話のキーと値の hash（Cursor）、または会話 DB と `-wal` の (size, mtime)（AGY）で行う。共有ファイル全体の hash は取らない。会話の数だけ 96MB を hash し直すことになるため。

### 5.2 Change Detector

- Cursor: `state.vscdb` / `-wal` の変更通知は「どこかの会話が変わった」合図としてだけ使う。合図が来たら `SELECT composerId, lastUpdatedAt FROM composerHeaders WHERE lastUpdatedAt > :watermark` で変わった会話を特定し、その会話だけを読む。
- AGY: `conversations/` の変更通知と、ホットファイルのポーリング（v3）で `-wal` の (size, mtime) を見る。

### 5.4 Session Aggregator

**自己観測の除外の前提（実測で確定）**

| 製品 | 会話 ID の変数 | ツールのシェル | MCP サーバー | 備考 |
|---|---|---|---|---|
| Claude Code | `CLAUDE_CODE_SESSION_ID` | 届く | **届く** | 起動時の値のまま。`/clear` 後は古い ID を指す |
| Codex | `CODEX_THREAD_ID` | 届く | 届かない | |
| AGY | `ANTIGRAVITY_CONVERSATION_ID` | 届く | 届かない | `ANTIGRAVITY_TRAJECTORY_ID` は別物 |
| Cursor | 不明 | 未確認 | 届かない（関連する変数なし） | |

- 常駐アプリ（Tauri）はどの製品の子プロセスでもないので、どの変数も届かない。**MVP では自己観測の除外を行わない**（v3 を維持）。
- 将来、MCP サーバーやフックとして Lens の一部を製品から起動させる場合の判定方法:
  1. Claude Code は環境変数の ID を使う。そのログが 600 秒以上更新されず、同じプロジェクトの別の top_level のログが 600 秒以内に更新されていれば、`/clear` などで切り替わったとみなしてそちらを使う。
  2. 変数が届かない製品は、その製品で、プロジェクト内の top_level のうち直近 600 秒以内に活動した最新の会話を「自分」とみなす。活動時刻は §2.3 / §2.4 の根拠を使う。Codex は guardian を候補から外す（§2.2）。
  3. この判定は並行作業中に別の会話を選ぶことがある。除外した会話を必ず UI に表示する（黙って消さない）。
- 状態の判定（§5.3）にはこの推定を使わない。v3 の「セッションの切り替えは推定しない」を維持する。

---

## 7. プライバシー（追加）

- 既定の redaction に、環境変数の代入形式を加える: `(?i)\b[A-Z0-9_]*(TOKEN|SECRET|PASSWORD|API_?KEY|CSRF)[A-Z0-9_]*\s*[=:]\s*\S+` の値の部分を伏せる。
  - 実例: ユーザーがエージェントに環境変数の一覧を出させ、その出力を別のエージェントの会話に貼ったため、`ANTIGRAVITY_CSRF_TOKEN=…` が Claude Code のログに残った。
- ツール出力の文字化け: Windows のエラーメッセージ（CP932）が、元ログの時点ですでに U+FFFD に置き換わっていることがある。これは正しい UTF-8 なので `invalid_utf8` にはならない。そのまま表示し、修復しようとしない。

---

## 8. テスト方針（追加）

- Cursor: 無関係な大きい値を含む `state.vscdb` で、上限より小さい会話が読めること。無関係な値が変わっても、その会話を読み直さないこと。
- Cursor: `composerHeaders.lastUpdatedAt` の更新だけで、その会話が dirty になること。
- AGY: ファイル名だけで一覧が作れること。`-wal` だけが更新されたときに活動時刻が進むこと。
- Codex: guardian_review の rollout が title・件数・「最新のセッション」の候補に入らないこと。
- 子プロセス: 親が stdin に何も書かない状態で、子プロセスを使う処理が時間内に終わること（Windows で必須）。
- 環境変数の分離: テストを各エージェントのシェルから実行すると `CLAUDE_CODE_SESSION_ID` などを引き継ぐ。共通の fixture で消す（v2 から維持）。

---

## 9. MVP の範囲

v3 §9 を維持する。Cursor と AGY は引き続き後回しだが、§2.3 / §2.4 / §5.1 / §5.2 で読み取りの方式は確定した。MVP 後に着手するときは、ここから始める。

## 10. 未検証・初期対象外（追加）

- Cursor をメインで使う PC での、`state.vscdb` の大きさと読み取り時間。この PC は無料プランで会話が少なく、composer 単位の読み取りで 161 件を判定するのに初回約 10 秒、2 回目以降（変更なし）約 2〜4 秒だった（Python 実装での値）。
- Cursor の subagent の親 composer ID の列（`composerHeaders` には見当たらない）。
- Cursor が MCP サーバー・ツールのシェルに渡す環境変数（この PC ではエージェントの利用上限に達して確認できなかった）。
- Codex の guardian 以外の automated スレッドの種類。
