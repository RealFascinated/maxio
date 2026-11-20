import { apiFetch } from './http'

export type JobKind = 'compressBucket' | 'housekeeping' | 'lifecycle' | 'orphanMetaRepair'
export type JobStatus = 'running' | 'completed' | 'cancelled' | 'failed'
export type JobTrigger = 'manual' | 'schedule'

export interface CompressBucketDetail {
  phase: 'objects' | 'versions'
  totalBytes: number
  compressed: number
  skipped: number
  failed: number
  bytesBefore: number
  bytesAfter: number
}

export interface HousekeepingDetail {
  uploadsRemoved: number
  tempFilesRemoved: number
}

export interface LifecycleDetail {
  expired: number
}

export interface OrphanMetaRepairDetail {
  removed: number
}

interface JobBase<K extends JobKind, D> {
  id: string
  kind: K
  scope: string | null
  status: JobStatus
  trigger: JobTrigger
  processed: number
  total: number | null
  detail: D
  error: string | null
  startedAt: string
  finishedAt: string | null
}

export type CompressBucketJob = JobBase<'compressBucket', CompressBucketDetail>
export type HousekeepingJob = JobBase<'housekeeping', HousekeepingDetail>
export type LifecycleJob = JobBase<'lifecycle', LifecycleDetail>
export type OrphanMetaRepairJob = JobBase<'orphanMetaRepair', OrphanMetaRepairDetail>

export type Job = CompressBucketJob | HousekeepingJob | LifecycleJob | OrphanMetaRepairJob

export interface ListJobsParams {
  kind?: JobKind
  scope?: string
  status?: JobStatus
  limit?: number
}

export interface StartJobInput {
  kind: JobKind
  scope?: string
}

export const JOB_KIND_LABELS: Record<JobKind, string> = {
  compressBucket: 'Compress bucket',
  housekeeping: 'Housekeeping',
  lifecycle: 'Lifecycle',
  orphanMetaRepair: 'Orphan metadata repair',
}

export async function listJobs(params: ListJobsParams = {}): Promise<{ jobs: Job[] }> {
  const query = new URLSearchParams()
  if (params.kind) query.set('kind', params.kind)
  if (params.scope) query.set('scope', params.scope)
  if (params.status) query.set('status', params.status)
  if (params.limit !== undefined) query.set('limit', String(params.limit))
  const suffix = query.size > 0 ? `?${query}` : ''
  return apiFetch<{ jobs: Job[] }>(`/api/jobs${suffix}`)
}

export async function getJob(id: string): Promise<{ job: Job }> {
  return apiFetch<{ job: Job }>(`/api/jobs/${encodeURIComponent(id)}`)
}

export async function startJob(input: StartJobInput): Promise<{ job: Job }> {
  return apiFetch<{ job: Job }>('/api/jobs', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function cancelJob(id: string): Promise<{ ok: true }> {
  return apiFetch<{ ok: true }>(`/api/jobs/${encodeURIComponent(id)}`, { method: 'DELETE' })
}
