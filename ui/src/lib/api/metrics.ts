import { apiFetch } from './http'

export interface CacheSnapshot {
  id: string
  name: string
  hits: number
  misses: number
  evictions: number
  dirtyBytes: number
  sizeBytes: number
  entries: number
  dirtyObjects: number
  maxSizeBytes: number
  writebackHalted: boolean
  enabled: boolean
}

export interface OperationSnapshot {
  operation: string
  count: number
  sumSeconds: number
  p50Seconds: number | null
  p95Seconds: number | null
  p99Seconds: number | null
}

export type StorageOpSnapshot = OperationSnapshot
export type MetadataOpSnapshot = OperationSnapshot

export interface ProcessSnapshot {
  residentMemoryBytes: number
  virtualMemoryBytes: number
  cpuUsagePercent: number
  openFds: number
  maxFds: number
}

export interface StorageTotalsSnapshot {
  bucketCount: number
  objectCount: number
  sizeBytes: number
}

export interface LatencySnapshot {
  windowSeconds: number
  readSeconds: number | null
  writeSeconds: number | null
}

export interface ThroughputSnapshot {
  windowSeconds: number
  readBytesPerSec: number
  writeBytesPerSec: number
}

export interface OpsTotalsSnapshot {
  windowSeconds: number
  readIops: number
  writeIops: number
  metaIops: number
}

export interface HttpTotalsSnapshot {
  totalRequests: number
  status2xx: number
  status3xx: number
  status4xx: number
  status5xx: number
  bytesRead: number
  bytesWritten: number
}

export interface Percentiles {
  p50: number
  p95: number
  p99: number
}

export interface LatencyPercentilesSnapshot {
  windowSeconds: number
  read: Percentiles | null
  write: Percentiles | null
  http: Percentiles | null
}

export interface BucketUsageSnapshot {
  name: string
  objectCount: number
  sizeBytes: number
}

export interface MetricsSnapshot {
  uptimeSeconds: number
  startedAt: number
  version: string
  storageTotals: StorageTotalsSnapshot
  throughput: ThroughputSnapshot
  latency: LatencySnapshot
  latencyPercentiles: LatencyPercentilesSnapshot
  opsTotals: OpsTotalsSnapshot
  activeClients: number
  http: HttpTotalsSnapshot
  buckets: BucketUsageSnapshot[]
  caches: CacheSnapshot[]
  storageOps: StorageOpSnapshot[]
  metadataOps: MetadataOpSnapshot[]
  process: ProcessSnapshot | null
}

export const METRICS_RANGES = ['15m', '1h', '6h', '24h', '7d', '30d'] as const
export type MetricsRange = (typeof METRICS_RANGES)[number]

export const SERIES_KEYS = [
  'readBps',
  'writeBps',
  'readIops',
  'writeIops',
  'metaIops',
  'reqPerSec',
  'req2xx',
  'req3xx',
  'req4xx',
  'req5xx',
  'activeClients',
  'readLatencyP50',
  'readLatencyP95',
  'readLatencyP99',
  'writeLatencyP50',
  'writeLatencyP95',
  'writeLatencyP99',
  'httpLatencyP50',
  'httpLatencyP95',
  'httpLatencyP99',
  'cpuPercent',
  'rssBytes',
  'openFds',
  'storageBytes',
  'objectCount',
] as const
export type SeriesKey = (typeof SERIES_KEYS)[number]

export interface MetricsHistory {
  range: MetricsRange
  resolutionSeconds: number
  from: number
  to: number
  /** Sample timestamps in unix ms; every series array is aligned with it and `null` marks a gap. */
  t: number[]
  series: Record<SeriesKey, (number | null)[]>
  cacheHitRates: Record<string, (number | null)[]>
}

export function fetchMetrics(): Promise<MetricsSnapshot> {
  return apiFetch<MetricsSnapshot>('/api/metrics')
}

export function fetchMetricsHistory(range: MetricsRange): Promise<MetricsHistory> {
  return apiFetch<MetricsHistory>(`/api/metrics/history?range=${range}`)
}
