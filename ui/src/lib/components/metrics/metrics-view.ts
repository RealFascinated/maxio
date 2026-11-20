import type { MetricsHistory, MetricsRange, MetricsSnapshot } from '$lib/api/metrics'

export type HistoryStatus = 'loading' | 'ready' | 'forbidden' | 'error'

/** Everything a section needs to render: the live snapshot plus the selected range's history. */
export interface MetricsView {
  snapshot: MetricsSnapshot
  history: MetricsHistory | undefined
  historyStatus: HistoryStatus
  range: MetricsRange
}

export const SECTIONS = [
  { id: 'overview', label: 'Overview' },
  { id: 'traffic', label: 'Traffic' },
  { id: 'storage', label: 'Storage' },
  { id: 'cache', label: 'Cache' },
  { id: 'operations', label: 'Operations' },
  { id: 'system', label: 'System' },
] as const
export type SectionId = (typeof SECTIONS)[number]['id']

export const REFRESH_OPTIONS = [
  { value: '5', label: '5s' },
  { value: '10', label: '10s' },
  { value: '30', label: '30s' },
  { value: '0', label: 'Off' },
] as const

export interface HealthIssue {
  severity: 'warning' | 'error'
  text: string
  section: SectionId
}

const lastWindowSum = (values: (number | null)[] | undefined, window = 6) =>
  (values ?? []).slice(-window).reduce<number>((sum, value) => sum + (value ?? 0), 0)

export function deriveHealth({ snapshot, history }: MetricsView): HealthIssue[] {
  const issues: HealthIssue[] = []

  const recentTotal = lastWindowSum(history?.series.reqPerSec)
  const recent5xx = lastWindowSum(history?.series.req5xx)
  const errorShare =
    recentTotal > 0
      ? recent5xx / recentTotal
      : snapshot.http.totalRequests > 0
        ? snapshot.http.status5xx / snapshot.http.totalRequests
        : 0
  if (errorShare >= 0.01) {
    issues.push({
      severity: errorShare >= 0.05 ? 'error' : 'warning',
      text: `5xx rate at ${(errorShare * 100).toFixed(1)}% of requests`,
      section: 'traffic',
    })
  }

  const httpP99 = snapshot.latencyPercentiles.http?.p99
  if (httpP99 != null && httpP99 > 1) {
    issues.push({
      severity: 'warning',
      text: `HTTP p99 latency is ${httpP99.toFixed(2)} s`,
      section: 'traffic',
    })
  }

  for (const cache of snapshot.caches) {
    if (cache.writebackHalted) {
      issues.push({ severity: 'error', text: `${cache.name} writeback halted`, section: 'cache' })
    }
    if (cache.enabled && cache.maxSizeBytes > 0 && cache.dirtyBytes / cache.maxSizeBytes >= 0.5) {
      issues.push({
        severity: 'warning',
        text: `${cache.name} dirty backlog at ${Math.round((cache.dirtyBytes / cache.maxSizeBytes) * 100)}% of capacity`,
        section: 'cache',
      })
    }
  }

  const { process } = snapshot
  if (process && process.maxFds > 0 && process.openFds / process.maxFds >= 0.8) {
    issues.push({
      severity: process.openFds / process.maxFds >= 0.95 ? 'error' : 'warning',
      text: `${process.openFds.toLocaleString()} of ${process.maxFds.toLocaleString()} file descriptors in use`,
      section: 'system',
    })
  }
  if (process && process.cpuUsagePercent >= 90) {
    issues.push({
      severity: 'warning',
      text: `CPU at ${process.cpuUsagePercent.toFixed(0)}%`,
      section: 'system',
    })
  }

  return issues
}
