// Mirrors architecture-v5 section 7 (the single type definition of IPC and NDJSON).

export type Agent = "claude_code" | "codex";
export type Completeness = "complete" | "partial" | "unsupported" | "unavailable";
export type StatusValue = "running" | "waiting_input" | "idle" | "error" | "unknown";
export type SessionKind = "top_level" | "subagent" | "fork" | "automated";
export type TimeSource = "event" | "file_mtime";
export type Confidence = "high" | "low";
export type SourceState =
  | "ok"
  | "not_installed"
  | "empty"
  | "access_denied"
  | "unsupported_schema"
  | "error"
  | "distro_stopped"
  | "helper_unavailable"
  | "helper_version_mismatch"
  | "unsupported";

export interface Diagnostic {
  code: string;
  count: number;
}

export interface Origin {
  kind: "windows" | "wsl";
  distro?: string;
}

export interface SessionProject {
  key: string;
  name: string;
  displayPath: string;
  attribution: "git_root" | "raw_path";
}

export interface ProjectActivity {
  key: string;
  name: string;
  origin: Origin;
  lastUpdatedAt?: string;
  lastUpdatedSource: TimeSource;
  updating: boolean;
  updatingConfidence?: Confidence;
  originAvailable: boolean;
  agents: Agent[];
  sessionUids: string[];
}

export interface UnifiedSession {
  sessionUid: string;
  nativeId: string;
  agent: Agent;
  sourceId: string;
  origin: Origin;
  lastActivityAt?: string;
  lastActivitySource: TimeSource;
  projects: SessionProject[];
  status: { value: StatusValue; reason: string; confidence: Confidence; stale: boolean; autoReview: boolean };
  currentTask: {
    title?: string;
    lastUserMessage?: string;
    lastAssistantMessage?: string;
    activeTool?: { name: string; input: string; callId?: string; startedAt?: string };
  };
  hierarchy: { parentUid?: string; childUids: string[]; kind: SessionKind };
  health: { completeness: Completeness; diagnostics: Diagnostic[] };
}

export interface SourceHealth {
  sourceId: string;
  agent?: Agent;
  origin: Origin;
  rootPath: string;
  state: SourceState;
  lastScanAt?: string;
  filesTracked: number;
  sessionsTotal: number;
  partialSessions: number;
  skippedOldFiles: number;
  topDiagnostics: Diagnostic[];
}

export interface Settings {
  version: 1;
  helperPath?: string;
  disabledDistros?: string[];
  sourceRoots?: Record<string, string>;
}

export type IpcErrorCode =
  | "not_ready"
  | "invalid_argument"
  | "project_not_found"
  | "distro_not_found"
  | "source_not_found"
  | "path_invalid"
  | "settings_save_failed";

export interface IpcError {
  code: IpcErrorCode;
  message: string;
}

export interface Detail {
  project: ProjectActivity;
  sessions: UnifiedSession[];
}

export type Ev =
  | { upserts: ProjectActivity[]; removes: string[]; order: string[] }
  | { key: string }
  | { sources: SourceHealth[] };
