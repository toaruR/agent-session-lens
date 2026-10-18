# セッション別トークン表示

右上のセッション一覧では、プロジェクト内の最新セッションだけを初期展開する。エージェント別グループは初期展開し、セッションの見出しにはタイトル・状態・更新時刻を表示する。ユーザーが操作した開閉状態は、そのプロジェクトを表示している間維持する。

セッション本文を展開したときだけ `get_session_tokens({ sessionUid })` を呼ぶ。折りたたまれたカード単体の計算は起動しないが、展開中の親カードの合算に必要な子孫ログは読む。開いたままログが更新された場合も再取得する。別プロジェクトに属する子孫の更新イベントも再取得の対象にする。処理はバックグラウンドで行い、読み込み中・記録なし・失敗を区別する。記録なしを 0 とみなさず、再試行できる。

`calc-session-tokens` の計算方式を Rust に実装した（Python 実行環境や参考リポジトリへの実行時依存はない）。カード内に「単体」を表示し、子孫があれば「子孫込み」を並記する。合算は子・孫以降を再帰的にたどり、source を含む `sessionUid` で重複・循環を除去する。一部の使用量が不明・取得失敗なら、合計を「判明分」と明示し、それぞれの件数と再試行ボタンを表示する。親の使用量が不明でも既知の子孫は集計する。

子孫の発見には、収集済みセッションの親子関係に加えて、Codex の `SubAgentActivity/started` と `spawn_agent` の出力、Claude の `subagents/agent-*.jsonl` とその入れ子ディレクトリ、AGY の `brain/<id>/.system_generated/logs/transcript.jsonl` の構造化された `conversationId` / `conversation_id` / `subagent_session_id` を使う。継承履歴の ordinal 境界がある場合、過去の起動イベントは除く。AGY は IDE/CLI の別 conversations ルートも探す。時間帯が近いだけの無関係なセッションは推測で合算しない。ログに親子関係や明示的な起動情報がない子孫は検出できない。

- Codex: 最後の `event_msg/token_count/info.total_token_usage` を採用する。入力はキャッシュを含むため、通常入力は入力からキャッシュ入力とキャッシュ作成分を引いた値。`reasoning_output_tokens` は出力の内数で加算しない。`subagent_history_start_ordinal` がある場合、それより前の継承レコードを除外する。
- Claude Code: `assistant.message.usage` を読み、同じ `message.id` は最後の値だけを数える。ID 欠落時は `uuid`、それも欠ける場合は行番号を使う。同一ファイル内の親/sidechain と `subagents/agent-*.jsonl` の子を分離する。
- Antigravity: 読み取り専用 SQLite 接続で `steps.metadata` の protobuf field 9 を集計する。使用量が取れない形式では `gen_metadata.data` の field 1 → field 4 を使う。両テーブルを合算しない。usage の field 2/3/4/5 は通常入力/出力/キャッシュ作成/キャッシュ入力。

合計はキャッシュ入力＋通常入力＋キャッシュ作成＋出力。料金やサブスクリプション使用枠への換算値ではない。

既知の集計値と子の参照は設定ディレクトリの `session-tokens.json`（version 2）に保存する（通常 `%APPDATA%\agent-session-lens\session-tokens.json`）。キーは source を含む `sessionUid`、検証情報はログのパス・サイズ・更新時刻、子ディレクトリ、AGY の WAL と transcript のサイズ・更新時刻。変更がなければ再起動後もログ全文を再集計しない。子孫込み合計自体は固定保存せず、各ノードのキャッシュを検証して毎回合算し、親ログが変わらない子孫の更新も反映する。計算途中でログが更新された場合は再試行し、安定した結果だけを一時ファイル＋rename で保存する。未知の値と失敗は保存しない。破損・旧バージョンのキャッシュは無視する。

ローカルは Engine が取り込みに使った正確なファイルを参照する。稼働中の WSL は収集元ルートを `\\wsl.localhost\<distro>\...` に変換して対象ファイルを探す。停止中・利用不可の WSL にはアクセスしない。元のログや DB は変更しない。

検証: `cargo test -p lens-core --test tokens`、`node --test src/*.test.ts`、`npm run build`、`cargo check -p agent-session-lens`。画面の遅延取得・開閉・再試行は `node scripts/verify-session-tokens.mjs`（Playwright が必要）で検証できる。既存の Edge を使う場合は `LENS_TEST_BROWSER=msedge` を設定する。
