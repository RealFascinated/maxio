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

export interface MetricsSnapshot {
  uptimeSeconds: number
  storageTotals: StorageTotalsSnapshot
  throughput: ThroughputSnapshot
  latency: LatencySnapshot
  opsTotals: OpsTotalsSnapshot
  activeClients: number
  caches: CacheSnapshot[]
  storageOps: StorageOpSnapshot[]
  metadataOps: MetadataOpSnapshot[]
  process: ProcessSnapshot | null
}

export async function fetchMetrics(): Promise<MetricsSnapshot> {
  return apiFetch('/api/metrics')
}
