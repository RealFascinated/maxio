import { apiFetch } from './http'

export interface OrphanMetaEntry {
  bucket: string
  key: string
  versionId?: string
}

export interface OrphanMetaScanResult {
  count: number
  orphans: OrphanMetaEntry[]
}

export async function scanOrphanMeta(): Promise<OrphanMetaScanResult> {
  return apiFetch<OrphanMetaScanResult>('/api/maintenance/orphan-meta')
}
