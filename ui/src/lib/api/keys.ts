export const authKeys = {
  all: ['auth'] as const,
  check: () => [...authKeys.all, 'check'] as const,
  config: () => [...authKeys.all, 'config'] as const,
}

export const bucketKeys = {
  all: ['buckets'] as const,
  list: () => [...bucketKeys.all, 'list'] as const,
  detail: (bucket: string) => [...bucketKeys.all, 'detail', bucket] as const,
}

export const objectKeys = {
  all: ['objects'] as const,
  list: (bucket: string, prefix: string, q?: string) =>
    [...objectKeys.all, 'list', bucket, prefix, q ?? ''] as const,
}

export const versionKeys = {
  all: ['versions'] as const,
  list: (bucket: string, key: string) => [...versionKeys.all, 'list', bucket, key] as const,
}

export const settingsKeys = {
  all: ['settings'] as const,
  versioning: (bucket: string) => [...settingsKeys.all, 'versioning', bucket] as const,
  publicAccess: (bucket: string) => [...settingsKeys.all, 'public', bucket] as const,
  cors: (bucket: string) => [...settingsKeys.all, 'cors', bucket] as const,
}

export const userKeys = {
  all: ['users'] as const,
  list: () => [...userKeys.all, 'list'] as const,
  policies: () => [...userKeys.all, 'policies'] as const,
  userPolicy: (username: string, policyName: string) =>
    [...userKeys.all, 'policy', username, policyName] as const,
}

export const metricsKeys = {
  all: ['metrics'] as const,
  snapshot: () => [...metricsKeys.all, 'snapshot'] as const,
}
