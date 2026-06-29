export type PolicyEffect = 'Allow' | 'Deny'

export type PolicyVariant = 'identity' | 'bucket'

/** Principal in bucket policies: "*" or { AWS: arn | arn[] } */
export type PrincipalSpec = '*' | { AWS: string | string[] }

export interface PolicyStatement {
  Sid?: string
  Effect: PolicyEffect
  Action: string[]
  Resource: string[]
  Principal?: PrincipalSpec
  Condition?: Record<string, unknown>
}

export interface PolicyDocument {
  Version: string
  Statement: PolicyStatement[]
}

export type PolicyEditorMode = 'builder' | 'json'
