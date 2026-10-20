import { useCallback, useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { relativeTime, updatingLabel } from "./format";
import { newestFirst } from "./sessionOrder";
import type { Agent, Detail, IpcError, ProjectActivity, SourceHealth, SourceState, StatusValue, TokenReport, TokenUsage, UnifiedSession } from "./types";

// All log-derived strings are rendered as React text nodes only (architecture-v5 section 9).

const AGENT_LABEL: Record<Agent, string> = { claude_code: "Claude Code", codex: "Codex", agy: "Antigravity" };
const RETRY_MS = 1000;
const NOTICE_MS = 3000;
// Editors reload settings.json on change; give the new window titles a moment to appear.
const TITLE_RELOAD_MS = 500;

const EDITOR_LABEL: Record<string, string> = {
  Code: "VS Code",
  "Code - Insiders": "VS Code Insiders",
  VSCodium: "VSCodium",
  "Code - OSS": "Code - OSS",
  Cursor: "Cursor",
  "Antigravity IDE": "Antigravity IDE",
};

const STATUS_LABEL: Record<StatusValue, string> = {
  running: "実行中",
  waiting_input: "入力待ち",
  idle: "アイドル",
  error: "エラー",
  unknown: "不明",
};

const STATUS_STYLE: Record<StatusValue, string> = {
  running: "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-200",
  waiting_input: "bg-sky-100 text-sky-800 dark:bg-sky-900/50 dark:text-sky-200",
  idle: "bg-zinc-200 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400",
  error: "bg-rose-100 text-rose-800 dark:bg-rose-900/50 dark:text-rose-200",
  unknown: "bg-amber-100 text-amber-800 dark:bg-amber-900/50 dark:text-amber-200",
};

const SOURCE_STATE_LABEL: Record<SourceState, string> = {
  ok: "正常",
  not_installed: "未導入",
  empty: "ログなし",
  access_denied: "アクセス拒否",
  unsupported_schema: "未対応形式",
  error: "エラー",
  distro_stopped: "ディストロ停止",
  helper_unavailable: "ヘルパー利用不可",
  helper_version_mismatch: "ヘルパー版不一致",
  unsupported: "未対応",
};

function useNow(intervalMs = 1000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

function isNotReady(e: unknown): boolean {
  return typeof e === "object" && e !== null && (e as IpcError).code === "not_ready";
}

/** Invoke a command, retrying while the backend answers not_ready. Returns null once cancelled. */
async function invokeRetry<T>(cmd: string, cancelled: () => boolean): Promise<T | null> {
  for (;;) {
    if (cancelled()) return null;
    try {
      return await invoke<T>(cmd);
    } catch (e) {
      if (!isNotReady(e)) throw e;
      await new Promise((r) => setTimeout(r, RETRY_MS));
    }
  }
}

interface ProjectsChanged {
  upserts: ProjectActivity[];
  removes: string[];
  order: string[];
}

function applyProjects(prev: ProjectActivity[], ev: ProjectsChanged): ProjectActivity[] {
  const m = new Map(prev.map((p) => [p.key, p]));
  for (const k of ev.removes) m.delete(k);
  for (const p of ev.upserts) m.set(p.key, p);
  return ev.order.map((k) => m.get(k)).filter((p): p is ProjectActivity => !!p);
}

function useLens(selected: string | null) {
  const [projects, setProjects] = useState<ProjectActivity[] | null>(null);
  const [health, setHealth] = useState<SourceHealth[] | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [tokenRevision, setTokenRevision] = useState(0);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;

  const loadDetail = useCallback((key: string | null) => {
    if (!key) {
      setDetail(null);
      return;
    }
    invoke<Detail>("get_project_detail", { key })
      .then((d) => {
        if (selectedRef.current === key) setDetail(d);
      })
      .catch(() => {
        if (selectedRef.current === key) setDetail(null);
      });
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let dead = false;
    const unlisteners: Array<() => void> = [];
    let tokenTimer: ReturnType<typeof setTimeout> | undefined;
    const cancelled = () => dead;
    const sub = async <P,>(name: string, fn: (p: P) => void) => {
      const u = await listen<P>(name, (e) => fn(e.payload));
      if (dead) u();
      else unlisteners.push(u);
    };
    void sub<ProjectsChanged>("projects_changed", (p) => setProjects((prev) => applyProjects(prev ?? [], p)));
    void sub<{ key: string }>("project_detail_changed", (p) => {
      // A descendant may belong to a different project from the open parent.
      if (tokenTimer === undefined) tokenTimer = setTimeout(() => {
        tokenTimer = undefined;
        if (!dead) setTokenRevision((revision) => revision + 1);
      }, 1000);
      if (p.key === selectedRef.current) loadDetail(p.key);
    });
    void sub<{ sources: SourceHealth[] }>("health_changed", (p) => setHealth(p.sources));
    invokeRetry<ProjectActivity[]>("list_projects", cancelled)
      .then((p) => p && !dead && setProjects(p))
      .catch(() => {});
    invokeRetry<SourceHealth[]>("get_health", cancelled)
      .then((h) => h && !dead && setHealth(h))
      .catch(() => {});
    return () => {
      dead = true;
      clearTimeout(tokenTimer);
      unlisteners.forEach((u) => u());
    };
  }, [loadDetail]);

  useEffect(() => {
    if (isTauri()) loadDetail(selected);
  }, [selected, loadDetail]);

  return { projects, health, detail, tokenRevision };
}

function ageSeconds(iso: string | undefined, now: number): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : Math.max(0, Math.floor((now - t) / 1000));
}

function StatusBadge({ s }: { s: UnifiedSession }) {
  const st = s.status;
  const label = STATUS_LABEL[st.value] + (st.stale ? " (stale)" : "") + (st.confidence === "low" ? "?" : "");
  return (
    <span className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium ${STATUS_STYLE[st.value]}`} title={st.reason}>
      {label}
      {st.autoReview && <span className="rounded bg-violet-200 px-1 text-violet-800 dark:bg-violet-900 dark:text-violet-200">自動審査中</span>}
    </span>
  );
}

type OnSelect = (key: string, e: MouseEvent) => void;

function ProjectRow({ p, selected, onSelect, now }: { p: ProjectActivity; selected: boolean; onSelect: OnSelect; now: number }) {
  const age = ageSeconds(p.lastUpdatedAt, now);
  const when = p.updating ? updatingLabel(true, p.updatingConfidence) : age === null ? "—" : relativeTime(age, p.lastUpdatedSource);
  return (
    <tr
      onClick={(e) => onSelect(p.key, e)}
      // Keep Shift/Ctrl+click from selecting text.
      onMouseDown={(e) => { if (e.shiftKey || e.ctrlKey) e.preventDefault(); }}
      title="Shift+クリックでフォルダを開く / Ctrl+クリックで開いているエディタ・エージェントを前面に"
      className={`cursor-pointer border-t border-zinc-200 hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-800/50 ${selected ? "bg-sky-50 dark:bg-sky-950/30" : ""}`}
    >
      <td className="py-1 pr-2 [overflow-wrap:anywhere] min-[900px]:py-2">
        <span className="font-medium">{p.name}</span>
        {p.origin.kind === "wsl" && <span className="ml-1 hidden text-xs text-zinc-500 min-[900px]:inline">(WSL: {p.origin.distro ?? ""})</span>}
        {!p.originAvailable && <span className="ml-2 hidden rounded bg-zinc-200 px-1.5 py-0.5 text-xs text-zinc-600 min-[900px]:inline dark:bg-zinc-800 dark:text-zinc-400">停止中</span>}
      </td>
      <td className={`py-1 pr-2 text-xs min-[900px]:py-2 ${p.updating ? "font-medium text-emerald-600" : "text-zinc-500"}`}>{when}</td>
      <td className="hidden py-2 pr-2 text-xs text-zinc-600 min-[900px]:table-cell dark:text-zinc-400">{p.agents.map((a) => AGENT_LABEL[a]).join(", ")}</td>
    </tr>
  );
}

function ProjectList({ projects, selected, onSelect, now }: { projects: ProjectActivity[]; selected: string | null; onSelect: OnSelect; now: number }) {
  return (
    <table className="w-full table-fixed border-collapse text-sm">
      <colgroup>
        <col />
        <col className="w-20 min-[900px]:w-28" />
        <col className="hidden w-48 min-[900px]:table-column" />
      </colgroup>
      <thead className="hidden min-[900px]:table-header-group">
        <tr className="text-left text-xs text-zinc-500">
          <th className="py-1 pr-2 font-medium">プロジェクト</th>
          <th className="w-28 py-1 pr-2 font-medium">更新</th>
          <th className="w-48 py-1 pr-2 font-medium">エージェント</th>
        </tr>
      </thead>
      <tbody>
        {projects.map((p) => (
          <ProjectRow key={p.key} p={p} selected={selected === p.key} onSelect={onSelect} now={now} />
        ))}
      </tbody>
    </table>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mb-2">
      <div className="text-xs text-zinc-500">{label}</div>
      <div className="break-words whitespace-pre-wrap">{children}</div>
    </div>
  );
}

function SessionTokens({ s, revision }: { s: UnifiedSession; revision: string }) {
  const [report, setReport] = useState<TokenReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let dead = false;
    setLoading(true);
    setError(null);
    invoke<TokenReport>("get_session_tokens", { sessionUid: s.sessionUid })
      .then((result) => { if (!dead) setReport(result); })
      .catch((e: unknown) => { if (!dead) setError(typeof e === "string" ? e : "トークン数を取得できませんでした"); })
      .finally(() => { if (!dead) setLoading(false); });
    return () => { dead = true; };
  }, [s.sessionUid, revision, attempt]);
  const partial = !!report && report.unknownCount + report.failedCount > 0;
  const hasChildren = !!report && report.subagentCount > 0;
  const rows: Array<[string, keyof TokenUsage]> = [
    ["合計", "totalTokens"], ["キャッシュ入力", "cachedInputTokens"], ["通常入力", "uncachedInputTokens"],
    ["キャッシュ作成", "cacheCreationInputTokens"], ["出力（推論を含む）", "outputTokens"],
  ];
  // Codex always logs cache_write_input_tokens as 0 and AGY never writes field 4: 0 there means "not recorded".
  const unrecorded = (usage: TokenUsage, key: keyof TokenUsage) =>
    key === "cacheCreationInputTokens" && s.agent !== "claude_code" && usage[key] === 0;
  const value = (usage: TokenUsage | null, key: keyof TokenUsage) =>
    !usage ? "不明" : unrecorded(usage, key) ? "記録なし" : usage[key].toLocaleString("ja-JP");
  return (
    <Field label="トークン数">
      <div aria-live="polite">
        {loading && <div className="text-xs text-zinc-500">計算中…</div>}
        {error && <div className="text-xs text-rose-600">{error}</div>}
        {!loading && !error && report && !report.session && <div className="text-xs text-zinc-500">不明（ログに使用量の記録がありません）</div>}
        {report && <>
          <table className="w-full text-xs tabular-nums">
            <thead><tr className="text-zinc-500"><th className="text-left font-normal">内訳</th><th className="text-right font-normal">単体</th>{hasChildren && <th className="text-right font-normal">子孫込み{partial ? "（判明分）" : ""}</th>}</tr></thead>
            <tbody>{rows.map(([label, key]) => <tr key={key} className={key === "totalTokens" ? "font-semibold" : ""}>
              <th scope="row" className="py-0.5 text-left font-normal">{label}</th>
              <td className="text-right">{value(report.session, key)}</td>
              {hasChildren && <td className="text-right">{value(report.includingSubagents, key)}</td>}
            </tr>)}</tbody>
          </table>
          {hasChildren && <div className="mt-1 text-xs text-zinc-500">子・孫以降 {report.subagentCount} セッションを含む</div>}
          {hasChildren && partial && <div className="text-xs text-amber-600">不明 {report.unknownCount} 件・取得失敗 {report.failedCount} 件（合計は判明分）</div>}
        </>}
        {!loading && (error || partial) && <button type="button" className="mt-1 text-xs text-sky-600 underline" onClick={() => setAttempt((a) => a + 1)}>再試行</button>}
      </div>
    </Field>
  );
}

function SessionCard({ s, now, latest, tokenRevision }: { s: UnifiedSession; now: number; latest: boolean; tokenRevision: string }) {
  const [userExpanded, setUserExpanded] = useState<boolean | null>(null);
  const expanded = userExpanded ?? latest;
  const age = ageSeconds(s.lastActivityAt, now);
  const t = s.currentTask;
  return (
    <details open={expanded} className="group/session mb-3 rounded border border-zinc-200 text-sm dark:border-zinc-800">
      <summary onClick={(e) => { e.preventDefault(); setUserExpanded(!expanded); }} className="cursor-pointer list-none rounded p-2 hover:bg-zinc-50 focus-visible:outline-2 focus-visible:outline-sky-500 dark:hover:bg-zinc-800 [&::-webkit-details-marker]:hidden">
        <div className="flex items-center gap-2">
          <span aria-hidden="true" className="text-xs text-zinc-500">{expanded ? "▼" : "▶"}</span>
          <StatusBadge s={s} />
          <span className="text-xs text-zinc-500">{s.hierarchy.kind}</span>
          <span className="ml-auto text-xs text-zinc-500">{age === null ? "—" : relativeTime(age, s.lastActivitySource)}</span>
        </div>
        <div className="mt-1 truncate font-medium" title={t.title ?? s.nativeId}>{t.title ?? s.nativeId}</div>
      </summary>
      {expanded && <div className="border-t border-zinc-200 p-2 dark:border-zinc-800">
        <SessionTokens s={s} revision={tokenRevision} />
        <Field label="状態の根拠">{`${s.status.reason} (確度 ${s.status.confidence})`}</Field>
        {t.activeTool && <Field label="実行中のツール">{`${t.activeTool.name}: ${t.activeTool.input}`}</Field>}
        <Field label="最初の依頼">{t.title ?? "—"}</Field>
        <Field label="直近の依頼">{t.lastUserMessage ?? "—"}</Field>
        <Field label="直近の応答">{t.lastAssistantMessage ?? "—"}</Field>
        <Field label="プロジェクト">{s.projects.length ? s.projects.map((p) => `${p.displayPath} [${p.attribution}]`).join("\n") : "帰属不明"}</Field>
        <Field label="健全性">
          {s.health.completeness}
          {s.health.diagnostics.length > 0 && " · " + s.health.diagnostics.map((d) => `${d.code}×${d.count}`).join(", ")}
        </Field>
        <Field label="ID">{s.sessionUid}</Field>
      </div>}
    </details>
  );
}

function AgentSessions({ sessions, now, revision }: { sessions: UnifiedSession[]; now: number; revision: number }) {
  const groups = new Map<Agent, UnifiedSession[]>();
  const ordered = newestFirst(sessions);
  const latestUid = ordered[0]?.sessionUid;
  const tokenRevision = String(revision);
  for (const session of ordered) {
    const group = groups.get(session.agent) ?? [];
    group.push(session);
    groups.set(session.agent, group);
  }

  return (
    <div className="space-y-3">
      {[...groups].map(([agent, agentSessions]) => (
        <details key={agent} open className="group rounded border border-zinc-200 dark:border-zinc-800">
          <summary className="flex cursor-pointer list-none items-center gap-2 rounded p-2 text-sm hover:bg-zinc-50 focus-visible:outline-2 focus-visible:outline-sky-500 dark:hover:bg-zinc-800 [&::-webkit-details-marker]:hidden">
            <span aria-hidden="true" className="text-xs text-zinc-500 transition-transform group-open:rotate-90">▶</span>
            <span className="font-semibold">{AGENT_LABEL[agent]}</span>
            <span className="ml-auto text-xs text-zinc-500">{agentSessions.length}件</span>
          </summary>
          <div className="border-t border-zinc-200 p-2 [&>div:last-child]:mb-0 dark:border-zinc-800">
            {agentSessions.map((s) => <SessionCard key={s.sessionUid} s={s} now={now} latest={s.sessionUid === latestUid} tokenRevision={tokenRevision} />)}
          </div>
        </details>
      ))}
    </div>
  );
}

function HealthPanel({ sources, now }: { sources: SourceHealth[]; now: number }) {
  // Every source is always listed with its state and counts (zeros included), so a format change is never hidden.
  return (
    <div className="grid grid-cols-1 gap-3 text-xs md:grid-cols-2">
      {sources.map((s) => {
        const age = ageSeconds(s.lastScanAt, now);
        return (
          <div key={s.sourceId} className="rounded border border-zinc-200 p-2 dark:border-zinc-800">
            <div className="flex justify-between">
              <span className="font-medium">{s.sourceId}</span>
              <span className={s.state === "ok" ? "text-emerald-600" : "text-amber-600"}>{SOURCE_STATE_LABEL[s.state]}</span>
            </div>
            <div className="truncate text-zinc-500" title={s.rootPath}>
              {s.rootPath}
            </div>
            <div>
              ファイル {s.filesTracked}（古いもの {s.skippedOldFiles} 件は対象外）· セッション {s.sessionsTotal} · partial {s.partialSessions}
            </div>
            <div>最終走査 {age === null ? "—" : relativeTime(age)}</div>
            <div className="text-zinc-500">{s.topDiagnostics.length > 0 ? s.topDiagnostics.map((d) => `${d.code}×${d.count}`).join(", ") : "診断なし"}</div>
          </div>
        );
      })}
    </div>
  );
}

function TitleSetupDialog({ editors, onAccept, onDecline }: { editors: string[]; onAccept: () => void; onDecline: () => void }) {
  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/40 p-4" onKeyDown={(e) => { if (e.key === "Escape") onDecline(); }}>
      <div role="dialog" aria-modal="true" aria-labelledby="title-setup-heading" className="max-w-md rounded-lg bg-white p-4 text-sm shadow-lg dark:bg-zinc-900">
        <h2 id="title-setup-heading" className="mb-2 font-semibold">ウィンドウタイトルにパスを表示しますか？</h2>
        <p className="mb-2">{editors.map((e) => EDITOR_LABEL[e] ?? e).join("、")} のウィンドウタイトルにフォルダのパスがないため、フォルダ名でしか照合できません（同名のフォルダを取り違えることがあります）。</p>
        <p className="mb-4 text-xs text-zinc-500">各エディタの settings.json（プロファイルを含む）の window.title に ⟨${"{rootPath}"}⟩ を追加します。変更前のファイルは .bak に保存します。再起動は不要です。</p>
        <div className="flex justify-end gap-2">
          <button type="button" className="rounded border border-zinc-300 px-3 py-1 hover:bg-zinc-50 dark:border-zinc-700 dark:hover:bg-zinc-800" onClick={onDecline}>今はしない</button>
          <button type="button" autoFocus className="rounded bg-sky-600 px-3 py-1 text-white hover:bg-sky-700" onClick={onAccept}>追加する</button>
        </div>
      </div>
    </div>
  );
}

export default function App() {
  const now = useNow();
  const [selected, setSelected] = useState<string | null>(null);
  const [healthExpanded, setHealthExpanded] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [titleSetup, setTitleSetup] = useState<{ key: string; editors: string[] } | null>(null);
  // Asked once per app run; declining falls back to folder-name matching.
  const titleSetupDeclined = useRef(false);
  const { projects, health, detail, tokenRevision } = useLens(selected);
  useEffect(() => {
    if (!notice) return;
    const id = setTimeout(() => setNotice(null), NOTICE_MS);
    return () => clearTimeout(id);
  }, [notice]);
  const fail = useCallback((err: unknown) => setNotice(typeof err === "string" ? err : "操作に失敗しました"), []);
  const focusProject = useCallback((key: string) => {
    invoke<number>("focus_project", { key })
      .then((n) => { if (n === 0) setNotice("このプロジェクトを開いているエディタ・エージェントは見つかりません"); })
      .catch(fail);
  }, [fail]);
  const onSelect = useCallback((key: string, e: MouseEvent) => {
    if (e.shiftKey) {
      invoke("reveal_project", { key }).catch(fail);
    } else if (e.ctrlKey) {
      invoke<string[]>("editors_without_title_path")
        .then((editors) => {
          if (editors.length > 0 && !titleSetupDeclined.current) setTitleSetup({ key, editors });
          else focusProject(key);
        })
        .catch(fail);
    } else {
      setSelected(key);
    }
  }, [fail, focusProject]);
  const acceptTitleSetup = () => {
    if (!titleSetup) return;
    const { key, editors } = titleSetup;
    setTitleSetup(null);
    invoke<string[]>("install_title_path", { editors })
      .then((errors) => {
        if (errors.length > 0) setNotice(`設定を書き換えられませんでした: ${errors.join(" / ")}`);
        setTimeout(() => focusProject(key), TITLE_RELOAD_MS);
      })
      .catch(fail);
  };
  const declineTitleSetup = () => {
    if (!titleSetup) return;
    titleSetupDeclined.current = true;
    setTitleSetup(null);
    focusProject(titleSetup.key);
  };
  const healthySources = health?.filter((source) => source.state === "ok").length ?? 0;

  if (!isTauri()) {
    return <div className="p-6 text-sm">このページは Tauri アプリの中で表示してください（npm run tauri dev）。</div>;
  }
  if (!projects) {
    return <div className="p-6 text-sm text-zinc-500">ログを読み込み中…</div>;
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden min-[900px]:gap-3 min-[900px]:p-3">
      <div className="flex min-h-0 flex-1 min-[900px]:gap-3">
        <main aria-label="プロジェクト一覧" className="min-w-0 flex-1 overflow-auto bg-white p-1 min-[900px]:rounded-lg min-[900px]:border min-[900px]:border-zinc-200 min-[900px]:p-3 dark:bg-zinc-900 min-[900px]:dark:border-zinc-800">
          <ProjectList projects={projects} selected={selected} onSelect={onSelect} now={now} />
        </main>
        <aside className="hidden w-[35%] max-w-96 shrink-0 overflow-auto rounded-lg border border-zinc-200 bg-white p-3 min-[900px]:block dark:border-zinc-800 dark:bg-zinc-900">
          {detail ? <AgentSessions key={detail.project.key} sessions={detail.sessions} now={now} revision={tokenRevision} /> : <div className="text-sm text-zinc-500">プロジェクトを選択してください</div>}
        </aside>
      </div>
      <footer className="hidden max-h-[30dvh] min-h-0 shrink-0 flex-col overflow-hidden rounded-lg border border-zinc-200 bg-white min-[900px]:flex dark:border-zinc-800 dark:bg-zinc-900">
        <button
          type="button"
          aria-expanded={healthExpanded}
          aria-controls="source-health-details"
          onClick={() => setHealthExpanded((expanded) => !expanded)}
          className="flex w-full shrink-0 cursor-pointer items-center gap-3 px-3 py-2 text-left text-xs hover:bg-zinc-50 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-sky-500 dark:hover:bg-zinc-800"
        >
          <span className="font-medium">ログ収集の状態</span>
          <span className="text-zinc-500">{health ? `正常 ${healthySources} / 全 ${health.length}` : "読み込み中…"}</span>
          <span className="ml-auto text-zinc-500">{healthExpanded ? "詳細を閉じる ▲" : "詳細を表示 ▼"}</span>
        </button>
        <div id="source-health-details" hidden={!healthExpanded} className="min-h-0 overflow-auto border-t border-zinc-200 p-3 dark:border-zinc-800">
          <HealthPanel sources={health ?? []} now={now} />
        </div>
      </footer>
      {titleSetup && <TitleSetupDialog editors={titleSetup.editors} onAccept={acceptTitleSetup} onDecline={declineTitleSetup} />}
      {notice && <div role="status" className="fixed bottom-3 left-1/2 -translate-x-1/2 rounded bg-zinc-800 px-3 py-1.5 text-xs text-white shadow dark:bg-zinc-200 dark:text-zinc-900">{notice}</div>}
    </div>
  );
}
