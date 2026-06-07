import { apiFetch } from './http'

export interface CacheSnapshot {
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

export interface StorageOpSnapshot {
  operation: string
  count: number
  sumSeconds: number
}

export interface MetadataOpSnapshot {
  operation: string
  count: number
  sumSeconds: number
}

export interface ProcessSnapshot {
  residentMemoryBytes: number
  virtualMemoryBytes: number
  cpuUsagePercent: number
  openFds: number
  maxFds: number
}

export interface MetricsSnapshot {
  uptimeSeconds: number
  cache: CacheSnapshot
  storageOps: StorageOpSnapshot[]
  metadataOps: MetadataOpSnapshot[]
  process: ProcessSnapshot | null
}

export async function fetchMetrics(): Promise<MetricsSnapshot> {
  return apiFetch('/api/metrics')
}
