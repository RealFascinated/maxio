import { parsePolicyDocument } from './document'
import { resourceMatchesBucket } from './resources'
import type { PolicyDocument, PolicyStatement, PolicyVariant, PrincipalSpec } from './types'

export interface PolicyValidationIssue {
  message: string
  warning?: boolean
}

function isSupportedVersion(version: string): boolean {
  return version === '2012-10-17' || version === '2008-10-17'
}

function validatePrincipal(principal: PrincipalSpec | undefined, index: number): PolicyValidationIssue[] {
  const issues: PolicyValidationIssue[] = []
  if (principal === undefined) {
    issues.push({ message: `Statement ${index + 1}: Principal is required for bucket policies` })
    return issues
  }
  if (principal === '*') return issues
  const aws = principal.AWS
  const arns = Array.isArray(aws) ? aws : [aws]
  if (!arns.length || arns.every((a) => !a.trim())) {
    issues.push({ message: `Statement ${index + 1}: Principal.AWS must not be empty` })
  }
  for (const arn of arns) {
    if (arn !== '*' && !arn.startsWith('arn:aws:iam::') && !arn.startsWith('arn:aws:sts::')) {
      issues.push({ message: `Statement ${index + 1}: invalid Principal ARN: ${arn}` })
    }
  }
  return issues
}

function validateStatement(
  stmt: PolicyStatement,
  index: number,
  variant: PolicyVariant,
  bucket?: string,
): PolicyValidationIssue[] {
  const issues: PolicyValidationIssue[] = []
  const n = index + 1

  if (!stmt.Action.length) {
    issues.push({ message: `Statement ${n}: at least one Action is required` })
  }
  if (!stmt.Resource.length) {
    issues.push({ message: `Statement ${n}: at least one Resource is required` })
  }

  if (variant === 'bucket') {
    issues.push(...validatePrincipal(stmt.Principal, index))
    if (bucket) {
      for (const resource of stmt.Resource) {
        if (!resourceMatchesBucket(resource, bucket)) {
          issues.push({
            message: `Statement ${n}: Resource ${resource} is not allowed for bucket ${bucket}`,
          })
        }
      }
    }
  } else if (stmt.Principal !== undefined) {
    issues.push({
      message: `Statement ${n}: Principal is ignored on identity policies`,
      warning: true,
    })
  }

  if (stmt.Condition !== undefined) {
    const cond = stmt.Condition
    if (typeof cond !== 'object' || cond === null || Array.isArray(cond)) {
      issues.push({ message: `Statement ${n}: Condition must be a JSON object` })
    }
  }

  return issues
}

export function validatePolicyDocument(
  doc: PolicyDocument,
  variant: PolicyVariant,
  bucket?: string,
): PolicyValidationIssue[] {
  const issues: PolicyValidationIssue[] = []

  if (!isSupportedVersion(doc.Version)) {
    issues.push({
      message: `Unsupported Version: ${doc.Version} (expected 2012-10-17 or 2008-10-17)`,
    })
  }
  if (!doc.Statement.length) {
    issues.push({ message: 'Policy must contain at least one statement' })
  }

  doc.Statement.forEach((stmt, i) => {
    issues.push(...validateStatement(stmt, i, variant, bucket))
  })

  return issues
}

export function validatePolicyText(
  text: string,
  variant: PolicyVariant,
  bucket?: string,
): { doc: PolicyDocument | null; issues: PolicyValidationIssue[] } {
  try {
    const doc = parsePolicyDocument(text)
    return { doc, issues: validatePolicyDocument(doc, variant, bucket) }
  } catch (e) {
    return {
      doc: null,
      issues: [{ message: e instanceof Error ? e.message : 'Invalid JSON' }],
    }
  }
}

export function isPolicyValid(
  result: { doc: PolicyDocument | null; issues: PolicyValidationIssue[] },
): result is { doc: PolicyDocument; issues: PolicyValidationIssue[] } {
  return result.doc !== null && !result.issues.some((i) => !i.warning)
}

export function blockingPolicyIssue(
  issues: PolicyValidationIssue[],
  fallback = 'Invalid policy',
): string {
  return issues.find((i) => !i.warning)?.message ?? fallback
}
