export function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

export function formatUptime(seconds: number): string {
  const total = Math.floor(seconds)
  const days = Math.floor(total / 86_400)
  const hours = Math.floor((total % 86_400) / 3_600)
  const minutes = Math.floor((total % 3_600) / 60)
  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0 || days > 0) parts.push(`${hours}h`)
  parts.push(`${minutes}m`)
  return parts.join(' ')
}

export function formatDuration(seconds: number): string {
  if (seconds < 0.001) return '<1 ms'
  if (seconds < 1) return `${(seconds * 1000).toFixed(1)} ms`
  if (seconds < 60) return `${seconds.toFixed(2)} s`
  const mins = Math.floor(seconds / 60)
  const secs = seconds % 60
  return `${mins}m ${secs.toFixed(0)}s`
}

export function formatLatency(seconds: number | null | undefined): { value: string; unit: string } {
  if (seconds == null || seconds <= 0) return { value: '0', unit: 'μs' }
  if (seconds < 0.001) {
    const micros = seconds * 1_000_000
    if (micros < 1) return { value: '<1', unit: 'μs' }
    return { value: Math.round(micros).toLocaleString(), unit: 'μs' }
  }
  if (seconds < 1) {
    const ms = seconds * 1_000
    return { value: parseFloat(ms.toFixed(1)).toLocaleString(), unit: 'ms' }
  }
  if (seconds < 60) {
    return { value: parseFloat(seconds.toFixed(2)).toLocaleString(), unit: 's' }
  }
  return { value: formatDuration(seconds), unit: '' }
}

export function formatIops(value: number): string {
  return Math.max(0, Math.round(value)).toLocaleString()
}

export function hitRate(hits: number, misses: number): string {
  const total = hits + misses
  if (total === 0) return '—'
  return `${((hits / total) * 100).toFixed(1)}%`
}

/** Snake-case id → Title Case label (e.g. `get_object` → `Get Object`). */
export function formatOperationName(id: string): string {
  return id
    .split('_')
    .map((part) => (part === 'iam' ? 'IAM' : part.charAt(0).toUpperCase() + part.slice(1)))
    .join(' ')
}

/** Snake-case cache metric id → Title Case label (e.g. `object_disk` → `Object Disk Cache`). */
export function formatMetricName(id: string): string {
  return `${formatOperationName(id)} Cache`
}
