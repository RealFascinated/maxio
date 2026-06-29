export type ActionCategory = 'bucket' | 'object' | 'multipart' | 'acl' | 'admin'

export interface S3ActionEntry {
  value: string
  category: ActionCategory
}

export const S3_ACTIONS: S3ActionEntry[] = [
  { value: 's3:ListBucket', category: 'bucket' },
  { value: 's3:GetBucketLocation', category: 'bucket' },
  { value: 's3:CreateBucket', category: 'bucket' },
  { value: 's3:DeleteBucket', category: 'bucket' },
  { value: 's3:ListBucketVersions', category: 'bucket' },
  { value: 's3:GetBucketVersioning', category: 'bucket' },
  { value: 's3:PutBucketVersioning', category: 'bucket' },
  { value: 's3:GetBucketCors', category: 'bucket' },
  { value: 's3:PutBucketCors', category: 'bucket' },
  { value: 's3:DeleteBucketCors', category: 'bucket' },
  { value: 's3:GetBucketPolicy', category: 'bucket' },
  { value: 's3:PutBucketPolicy', category: 'bucket' },
  { value: 's3:DeleteBucketPolicy', category: 'bucket' },
  { value: 's3:GetLifecycleConfiguration', category: 'bucket' },
  { value: 's3:PutLifecycleConfiguration', category: 'bucket' },
  { value: 's3:ListBucketMultipartUploads', category: 'multipart' },
  { value: 's3:GetObject', category: 'object' },
  { value: 's3:PutObject', category: 'object' },
  { value: 's3:DeleteObject', category: 'object' },
  { value: 's3:GetObjectVersion', category: 'object' },
  { value: 's3:DeleteObjectVersion', category: 'object' },
  { value: 's3:AbortMultipartUpload', category: 'multipart' },
  { value: 's3:ListMultipartUploadParts', category: 'multipart' },
  { value: 's3:GetBucketAcl', category: 'acl' },
  { value: 's3:PutBucketAcl', category: 'acl' },
  { value: 's3:GetObjectAcl', category: 'acl' },
  { value: 's3:PutObjectAcl', category: 'acl' },
  { value: 's3:ListAllMyBuckets', category: 'admin' },
  { value: 's3:*', category: 'admin' },
  { value: '*', category: 'admin' },
]

export const ACTION_PRESETS: { id: string; label: string; actions: string[] }[] = [
  {
    id: 'read-only',
    label: 'Read only',
    actions: ['s3:ListBucket', 's3:GetObject'],
  },
  {
    id: 'read-write',
    label: 'Read / write',
    actions: ['s3:ListBucket', 's3:GetObject', 's3:PutObject', 's3:DeleteObject'],
  },
  {
    id: 'full-s3',
    label: 'Full S3',
    actions: ['s3:*'],
  },
]

const CATEGORY_LABELS: Record<ActionCategory, string> = {
  bucket: 'Bucket',
  object: 'Object',
  multipart: 'Multipart',
  acl: 'ACL',
  admin: 'Admin',
}

export function actionsByCategory(): { category: ActionCategory; label: string; actions: S3ActionEntry[] }[] {
  const map = new Map<ActionCategory, S3ActionEntry[]>()
  for (const entry of S3_ACTIONS) {
    const list = map.get(entry.category) ?? []
    list.push(entry)
    map.set(entry.category, list)
  }
  return (['bucket', 'object', 'multipart', 'acl', 'admin'] as ActionCategory[]).map((category) => ({
    category,
    label: CATEGORY_LABELS[category],
    actions: map.get(category) ?? [],
  }))
}
