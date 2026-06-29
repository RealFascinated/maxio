import { IDENTITY_READ_WRITE_ACTIONS, identityReadWriteResources, objectArn } from './resources'
import type { PolicyDocument, PolicyStatement, PrincipalSpec } from './types'

function normalizeStringArray(value: unknown): string[] {
  if (typeof value === 'string') return value ? [value] : []
  if (Array.isArray(value)) {
    return value.filter((v): v is string => typeof v === 'string')
  }
  return []
}

function normalizePrincipal(raw: unknown): PrincipalSpec | undefined {
  if (raw === '*') return '*'
  if (typeof raw === 'object' && raw !== null && 'AWS' in raw) {
    const aws = (raw as { AWS: unknown }).AWS
    if (typeof aws === 'string') return { AWS: aws }
    if (Array.isArray(aws)) {
      const arns = aws.filter((v): v is string => typeof v === 'string')
      if (arns.length) return { AWS: arns }
    }
  }
  return undefined
}

function normalizeStatement(raw: Record<string, unknown>): PolicyStatement {
  const effect = raw.Effect === 'Deny' ? 'Deny' : 'Allow'
  const stmt: PolicyStatement = {
    Effect: effect,
    Action: normalizeStringArray(raw.Action),
    Resource: normalizeStringArray(raw.Resource),
  }
  if (typeof raw.Sid === 'string' && raw.Sid.trim()) stmt.Sid = raw.Sid.trim()
  const principal = normalizePrincipal(raw.Principal)
  if (principal !== undefined) stmt.Principal = principal
  if (raw.Condition !== undefined && typeof raw.Condition === 'object' && raw.Condition !== null) {
    stmt.Condition = raw.Condition as Record<string, unknown>
  }
  return stmt
}

export function parsePolicyDocument(text: string): PolicyDocument {
  const parsed = JSON.parse(text) as Record<string, unknown>
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Policy must be a JSON object')
  }
  const version =
    typeof parsed.Version === 'string' && parsed.Version.trim()
      ? parsed.Version.trim()
      : '2012-10-17'
  const rawStatements = parsed.Statement
  const statements: PolicyStatement[] = []
  if (Array.isArray(rawStatements)) {
    for (const item of rawStatements) {
      if (item && typeof item === 'object') {
        statements.push(normalizeStatement(item as Record<string, unknown>))
      }
    }
  } else if (rawStatements && typeof rawStatements === 'object') {
    statements.push(normalizeStatement(rawStatements as Record<string, unknown>))
  }
  return { Version: version, Statement: statements }
}

export function serializePolicyDocument(doc: PolicyDocument, pretty = false): string {
  const statements = doc.Statement.map((stmt) => {
    const out: Record<string, unknown> = {
      Effect: stmt.Effect,
      Action: stmt.Action.length === 1 ? stmt.Action[0] : stmt.Action,
      Resource: stmt.Resource.length === 1 ? stmt.Resource[0] : stmt.Resource,
    }
    if (stmt.Sid) out.Sid = stmt.Sid
    if (stmt.Principal !== undefined) out.Principal = stmt.Principal
    if (stmt.Condition !== undefined) out.Condition = stmt.Condition
    return out
  })
  const payload = { Version: doc.Version || '2012-10-17', Statement: statements }
  return JSON.stringify(payload, null, pretty ? 2 : undefined)
}

export function prettyPolicyJson(text: string): string | null {
  try {
    return serializePolicyDocument(parsePolicyDocument(text), true)
  } catch {
    return null
  }
}

export function newStatement(variant: 'identity' | 'bucket', bucket?: string): PolicyStatement {
  const stmt: PolicyStatement = {
    Effect: 'Allow',
    Action:
      variant === 'identity'
        ? [...IDENTITY_READ_WRITE_ACTIONS]
        : ['s3:GetObject'],
    Resource: bucket
      ? variant === 'bucket'
        ? [objectArn(bucket)]
        : [...identityReadWriteResources(bucket)]
      : [],
  }
  if (variant === 'bucket') stmt.Principal = '*'
  return stmt
}
