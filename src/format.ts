// Pure display helpers (architecture-v5 section 4).

export function relativeTime(ageS: number, source?: "event" | "file_mtime"): string {
  const base = ageS < 60 ? "1分未満" : `${Math.floor(ageS / 60)}分前`;
  return source === "file_mtime" ? `~${base}` : base;
}

export function updatingLabel(updating: boolean, confidence?: "high" | "low"): string {
  if (!updating) return "";
  return confidence === "low" ? "更新中?" : "更新中";
}
