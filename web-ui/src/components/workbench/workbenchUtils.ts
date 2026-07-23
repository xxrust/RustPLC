export function formatTime(value?: string): string {
  if (!value) return 'Not recorded';
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString();
}

export function shortCommit(value?: string): string {
  return value ? value.slice(0, 8) : 'unknown';
}
