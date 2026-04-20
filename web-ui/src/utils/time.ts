export function formatTimestamp(
  raw?: string | null,
  fallbackMs?: number
): string {
  if (raw) {
    const numericRaw = Number(raw);
    if (Number.isFinite(numericRaw) && raw.trim() !== '') {
      return new Date(numericRaw).toLocaleString();
    }

    const parsed = new Date(raw);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toLocaleString();
    }
  }

  if (typeof fallbackMs === 'number' && Number.isFinite(fallbackMs)) {
    return new Date(fallbackMs).toLocaleString();
  }

  return '-';
}
