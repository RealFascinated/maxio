import { base } from '$app/paths'

export const routes = {
  home: () => `${base}/`,
  users: () => `${base}/users`,
  metrics: () => `${base}/metrics`,
  serverSettings: () => `${base}/settings`,
  bucket: (bucket: string, prefix = '') => bucketObjectsUrl(bucket, prefix),
  bucketSettings: (bucket: string) => `${base}/buckets/${encodeURIComponent(bucket)}/settings`,
} as const

/** S3 prefix (`folder/sub/`) → URL path segment(s). */
export function prefixToPath(prefix: string): string {
  return prefix.replace(/\/$/, '')
}

/** URL path segment(s) → S3 prefix with trailing slash. */
export function pathToPrefix(path?: string): string {
  if (!path) return ''
  return path.endsWith('/') ? path : `${path}/`
}

export function bucketObjectsUrl(bucket: string, prefix = ''): string {
  const bucketPath = `${base}/buckets/${encodeURIComponent(bucket)}`
  const path = prefixToPath(prefix)
  if (!path) return bucketPath
  return `${bucketPath}/${path.split('/').map(encodeURIComponent).join('/')}`
}

export function isRootOnlyPath(pathname: string): boolean {
  return (
    pathname === routes.users() ||
    pathname === routes.metrics() ||
    pathname === routes.serverSettings()
  )
}

export function isBucketsNavActive(pathname: string): boolean {
  return !isRootOnlyPath(pathname)
}
