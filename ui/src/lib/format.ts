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

export function hitRate(hits: number, misses: number): string {
  const total = hits + misses
  if (total === 0) return '—'
  return `${((hits / total) * 100).toFixed(1)}%`
}
