export function bucketArn(bucket: string): string {
  return `arn:aws:s3:::${bucket}`
}

export function objectArn(bucket: string, key = '*'): string {
  return key === '*' ? `arn:aws:s3:::${bucket}/*` : `arn:aws:s3:::${bucket}/${key}`
}

export const ALL_BUCKETS_ARN = 'arn:aws:s3:::*'

/** List + get/put/delete objects in a single bucket (identity policies). */
export const IDENTITY_READ_WRITE_ACTIONS = [
  's3:ListBucket',
  's3:GetObject',
  's3:PutObject',
  's3:DeleteObject',
] as const

export function identityReadWriteResources(bucket: string): [string, string] {
  return [bucketArn(bucket), objectArn(bucket)]
}

function globMatch(pattern: string, value: string): boolean {
  if (pattern === '*') return true
  if (!pattern.includes('*') && !pattern.includes('?')) return pattern === value
  const parts: { type: 'lit' | 'star' | 'q'; value?: string }[] = []
  let lit = ''
  for (const ch of pattern) {
    if (ch === '*') {
      if (lit) parts.push({ type: 'lit', value: lit })
      lit = ''
      parts.push({ type: 'star' })
    } else if (ch === '?') {
      if (lit) parts.push({ type: 'lit', value: lit })
      lit = ''
      parts.push({ type: 'q' })
    } else {
      lit += ch
    }
  }
  if (lit) parts.push({ type: 'lit', value: lit })

  function rec(pi: number, s: string): boolean {
    if (pi >= parts.length) return s.length === 0
    const part = parts[pi]
    if (part.type === 'lit') {
      const v = part.value!
      return s.startsWith(v) && rec(pi + 1, s.slice(v.length))
    }
    if (part.type === 'q') {
      if (!s.length) return false
      return rec(pi + 1, s.slice(1))
    }
    if (pi === parts.length - 1) return true
    for (let i = 0; i <= s.length; i++) {
      if (rec(pi + 1, s.slice(i))) return true
    }
    return false
  }
  return rec(0, value)
}

export function resourceMatchesBucket(resource: string, bucket: string): boolean {
  if (resource === '*') return true
  const prefix = 'arn:aws:s3:::'
  if (!resource.startsWith(prefix)) return false
  const rest = resource.slice(prefix.length)
  if (!rest) return false
  const bucketPart = rest.split('/')[0] ?? rest
  if (bucketPart === bucket) return true
  return globMatch(bucketPart, bucket)
}
