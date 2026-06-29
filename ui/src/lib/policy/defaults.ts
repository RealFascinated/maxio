import { serializePolicyDocument } from './document'
import { IDENTITY_READ_WRITE_ACTIONS, objectArn } from './resources'
import type { PolicyDocument } from './types'

export function defaultIdentityPolicy(): string {
  const doc: PolicyDocument = {
    Version: '2012-10-17',
    Statement: [
      {
        Effect: 'Allow',
        Action: [...IDENTITY_READ_WRITE_ACTIONS],
        Resource: [],
      },
    ],
  }
  return serializePolicyDocument(doc, true)
}

export function defaultBucketPolicy(bucket: string): string {
  const doc: PolicyDocument = {
    Version: '2012-10-17',
    Statement: [
      {
        Effect: 'Allow',
        Principal: '*',
        Action: ['s3:GetObject'],
        Resource: [objectArn(bucket)],
      },
    ],
  }
  return serializePolicyDocument(doc, true)
}
