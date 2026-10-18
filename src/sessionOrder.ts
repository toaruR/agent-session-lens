import type { UnifiedSession } from "./types";

export function newestFirst(sessions: UnifiedSession[]): UnifiedSession[] {
  const time = (s: UnifiedSession) => Date.parse(s.lastActivityAt ?? "") || 0;
  return [...sessions].sort((a, b) => time(b) - time(a) || a.sessionUid.localeCompare(b.sessionUid));
}
