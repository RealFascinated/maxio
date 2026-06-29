import { apiFetch } from './http'

export interface EnabledResponse { enabled: boolean }
export interface PublicAccessResponse { read: boolean; list: boolean }

export async function getVersioning(bucket: string): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/versioning`)
}

export async function setVersioning(bucket: string, enabled: boolean): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/versioning`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  })
}

export async function getPublicAccess(bucket: string): Promise<PublicAccessResponse> {
  return apiFetch<PublicAccessResponse>(`/api/buckets/${encodeURIComponent(bucket)}/public`)
}

export async function setPublicAccess(bucket: string, read: boolean, list: boolean): Promise<PublicAccessResponse> {
  return apiFetch<PublicAccessResponse>(`/api/buckets/${encodeURIComponent(bucket)}/public`, {
    method: 'PUT',
    body: JSON.stringify({ read, list }),
  })
}

export async function getCors(bucket: string): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/cors`)
}

export async function setCors(bucket: string, enabled: boolean): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/cors`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  })
}

export type LifecycleAction =
  | { type: 'expire_objects'; days: number }
  | { type: 'noncurrent_version_expiration'; noncurrent_days: number }

export interface LifecycleRule {
  id: string
  enabled: boolean
  prefix?: string
  actions: LifecycleAction[]
}

export interface LifecycleResponse {
  rules: LifecycleRule[]
}

export async function getLifecycle(bucket: string): Promise<LifecycleResponse> {
  return apiFetch<LifecycleResponse>(`/api/buckets/${encodeURIComponent(bucket)}/lifecycle`)
}

export async function setLifecycle(bucket: string, rules: LifecycleRule[]): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/lifecycle`, {
    method: 'PUT',
    body: JSON.stringify({ rules }),
  })
}

export interface BucketPolicyResponse {
  document: string | null
}

export async function getBucketPolicy(bucket: string): Promise<BucketPolicyResponse> {
  return apiFetch<BucketPolicyResponse>(`/api/buckets/${encodeURIComponent(bucket)}/policy`)
}

export async function putBucketPolicy(bucket: string, document: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/policy`, {
    method: 'PUT',
    body: JSON.stringify({ document }),
  })
}

export async function deleteBucketPolicy(bucket: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/policy`, {
    method: 'DELETE',
  })
}
