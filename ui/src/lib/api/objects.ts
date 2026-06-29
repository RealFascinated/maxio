import { apiFetch, ApiError, encodeObjectKey } from './http'
import { notifyUnauthorized, SessionExpiredError } from './session'
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

export type ObjectListSort = 'name' | 'size' | 'modified' | 'type'
export type SortOrder = 'asc' | 'desc'

export async function listObjects(
  bucket: string,
  prefix: string,
  startAfter?: string,
  q?: string,
  sort: ObjectListSort = 'name',
  order: SortOrder = 'asc',
): Promise<ObjectsResponse> {
  const params = new URLSearchParams({ prefix, delimiter: '/' })
  if (startAfter) {
    params.set('start_after', startAfter)
  }
  const trimmed = q?.trim()
  if (trimmed) {
    params.set('q', trimmed)
  }
  if (sort !== 'name') {
    params.set('sort', sort)
  }
  if (order !== 'asc') {
    params.set('order', order)
  }
  return apiFetch<ObjectsResponse>(`/api/buckets/${encodeURIComponent(bucket)}/objects?${params}`)
}

export async function uploadObject(
  bucket: string,
  key: string,
  file: File,
  onProgress?: (loaded: number, total: number) => void,
): Promise<{ ok: boolean }> {
  const contentType = file.type || guessContentType(file.name)
  const url = `/api/buckets/${encodeURIComponent(bucket)}/upload/${encodeObjectKey(key)}`

  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('PUT', url)
    xhr.withCredentials = true
    if (contentType) xhr.setRequestHeader('Content-Type', contentType)

    xhr.upload.onprogress = (event) => {
      if (event.lengthComputable) onProgress?.(event.loaded, event.total)
    }

    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) resolve({ ok: true })
      else if (xhr.status === 401 && notifyUnauthorized(url)) {
        reject(new SessionExpiredError())
      } else {
        reject(new ApiError(`Upload failed (${xhr.status})`, xhr.status))
      }
    }
    xhr.onerror = () => reject(new Error('Upload failed'))
    xhr.send(file)
  })
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
  return apiFetch<DeleteObjectsResult>(`/api/buckets/${encodeURIComponent(bucket)}/objects/delete`, {
    method: 'POST',
    body: JSON.stringify({ keys }),
  })
}

export async function createFolder(bucket: string, name: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/folders`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}

export interface FolderDeletePreview {
  count: number
  sizeBytes: number
}

export async function previewFolderDelete(bucket: string, names: string[]): Promise<FolderDeletePreview> {
  return apiFetch<FolderDeletePreview>(`/api/buckets/${encodeURIComponent(bucket)}/folders/preview`, {
    method: 'POST',
    body: JSON.stringify({ names }),
  })
}

export async function deleteFolder(bucket: string, name: string): Promise<{ ok: boolean; deleted: number }> {
  return apiFetch<{ ok: boolean; deleted: number }>(`/api/buckets/${encodeURIComponent(bucket)}/folders`, {
    method: 'DELETE',
    body: JSON.stringify({ name }),
  })
}

export interface DeleteFoldersResult {
  deleted: number
  failed: string[]
}

export async function deleteFolders(bucket: string, prefixes: string[]): Promise<DeleteFoldersResult> {
  const failed: string[] = []
  let deleted = 0
  for (const prefix of prefixes) {
    try {
      const result = await deleteFolder(bucket, prefix)
      deleted += result.deleted
    } catch (err) {
      console.error('deleteFolder failed:', prefix, err)
      failed.push(prefix)
    }
  }
  return { deleted, failed }
}

export async function presignObject(bucket: string, key: string, expires: number): Promise<{ url: string }> {
  return apiFetch<{ url: string }>(`/api/buckets/${encodeURIComponent(bucket)}/presign/${encodeObjectKey(key)}?expires=${expires}`)
}

export function downloadUrl(bucket: string, key: string): string {
  return `/api/buckets/${encodeURIComponent(bucket)}/download/${encodeObjectKey(key)}`
}
