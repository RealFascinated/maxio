import { apiFetch } from './http'

export interface OrphanMetaEntry {
  bucket: string
  key: string
  versionId?: string
}

export interface OrphanMetaScanResult {
  asyncMetaWrite: boolean
  count: number
  orphans: OrphanMetaEntry[]
}

export interface OrphanMetaRepairResult {
  removed: number
}

export async function scanOrphanMeta(): Promise<OrphanMetaScanResult> {
  return apiFetch<OrphanMetaScanResult>('/api/maintenance/orphan-meta')
}

export async function repairOrphanMeta(): Promise<OrphanMetaRepairResult> {
  return apiFetch<OrphanMetaRepairResult>('/api/maintenance/orphan-meta/repair', {
    method: 'POST',
  })
}
