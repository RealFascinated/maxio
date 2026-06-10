import { apiFetch, encodeObjectKey } from './http'
import { guessContentType } from '$lib/mime'

export interface S3File {
  key: string
  size: number
  lastModified: string
  etag: string
  contentType: string
}

export interface ObjectDetail extends S3File {
  versionId?: string | null
  isDeleteMarker?: boolean
  tags: Record<string, string>
}

export interface ObjectsResponse {
  files: S3File[]
  prefixes: string[]
  nextContinuationToken?: string | null
}

export async function listObjects(
  bucket: string,
  prefix: string,
  startAfter?: string,
  q?: string,
): Promise<ObjectsResponse> {
  const params = new URLSearchParams({ prefix, delimiter: '/' })
  if (startAfter) {
    params.set('start_after', startAfter)
  }
  const trimmed = q?.trim()
  if (trimmed) {
    params.set('q', trimmed)
  }
  return apiFetch<ObjectsResponse>(`/api/buckets/${encodeURIComponent(bucket)}/objects?${params}`)
}

export async function uploadObject(bucket: string, key: string, file: File): Promise<{ ok: boolean }> {
  const contentType = file.type || guessContentType(file.name)
  const res = await fetch(`/api/buckets/${encodeURIComponent(bucket)}/upload/${encodeObjectKey(key)}`, {
    method: 'PUT',
    body: file,
    credentials: 'same-origin',
    headers: contentType ? { 'Content-Type': contentType } : undefined,
  })
  if (!res.ok) throw new Error(`Upload failed (${res.status})`)
  return { ok: true }
}

export async function getObjectDetail(bucket: string, key: string): Promise<ObjectDetail> {
  return apiFetch<ObjectDetail>(`/api/buckets/${encodeURIComponent(bucket)}/objects/${encodeObjectKey(key)}`)
}

export async function deleteObject(bucket: string, key: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/objects/${encodeObjectKey(key)}`, { method: 'DELETE' })
}

export interface DeleteObjectsResult {
  deleted: number
  failed: string[]
}

export async function deleteObjects(bucket: string, keys: string[]): Promise<DeleteObjectsResult> {
  const failed: string[] = []
  for (const key of keys) {
    try {
      await deleteObject(bucket, key)
    } catch (err) {
      console.error('deleteObject failed:', key, err)
      failed.push(key)
    }
  }
  return { deleted: keys.length - failed.length, failed }
}

export async function createFolder(bucket: string, name: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/folders`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}

export async function presignObject(bucket: string, key: string, expires: number): Promise<{ url: string }> {
  return apiFetch<{ url: string }>(`/api/buckets/${encodeURIComponent(bucket)}/presign/${encodeObjectKey(key)}?expires=${expires}`)
}

export function downloadUrl(bucket: string, key: string): string {
  return `/api/buckets/${encodeURIComponent(bucket)}/download/${encodeObjectKey(key)}`
}
