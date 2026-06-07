import { apiFetch } from './http'

export interface AccessKeySummary {
  accessKeyId: string
  status: string
  createdAt: string
}

export interface IamUserSummary {
  username: string
  userId: string
  createdAt: string
  accessKeys: AccessKeySummary[]
  attachedPolicies: string[]
  inlinePolicies: string[]
}

export interface UsersListResponse {
  users: IamUserSummary[]
}

export interface CreateUserResponse {
  ok: boolean
  username: string
  userId: string
  accessKey?: {
    accessKeyId: string
    secretAccessKey: string
  }
}

export interface CreateKeyResponse {
  ok: boolean
  accessKeyId: string
  secretAccessKey: string
}

export interface ManagedPolicySummary {
  name: string
  policyId: string
  arn: string
}

export interface PoliciesListResponse {
  policies: ManagedPolicySummary[]
}

export async function listUsers(): Promise<UsersListResponse> {
  return apiFetch('/api/users')
}

export async function createUser(username: string): Promise<CreateUserResponse> {
  return apiFetch('/api/users', {
    method: 'POST',
    body: JSON.stringify({ username }),
  })
}

export async function deleteUser(username: string): Promise<{ ok: boolean }> {
  return apiFetch(`/api/users/${encodeURIComponent(username)}`, { method: 'DELETE' })
}

export async function createUserKey(username: string): Promise<CreateKeyResponse> {
  return apiFetch(`/api/users/${encodeURIComponent(username)}/keys`, { method: 'POST' })
}

export async function deleteUserKey(username: string, accessKeyId: string): Promise<{ ok: boolean }> {
  return apiFetch(
    `/api/users/${encodeURIComponent(username)}/keys/${encodeURIComponent(accessKeyId)}`,
    { method: 'DELETE' },
  )
}

export async function getUserPolicy(username: string, policyName: string): Promise<{ policyName: string; document: string }> {
  return apiFetch(
    `/api/users/${encodeURIComponent(username)}/policies/${encodeURIComponent(policyName)}`,
  )
}

export async function putUserPolicy(username: string, policyName: string, document: string): Promise<{ ok: boolean }> {
  return apiFetch(
    `/api/users/${encodeURIComponent(username)}/policies/${encodeURIComponent(policyName)}`,
    {
      method: 'PUT',
      body: JSON.stringify({ document }),
    },
  )
}

export async function deleteUserPolicy(username: string, policyName: string): Promise<{ ok: boolean }> {
  return apiFetch(
    `/api/users/${encodeURIComponent(username)}/policies/${encodeURIComponent(policyName)}`,
    { method: 'DELETE' },
  )
}

export async function listPolicies(): Promise<PoliciesListResponse> {
  return apiFetch('/api/policies')
}

export async function createPolicy(name: string, document: string): Promise<{ ok: boolean; name: string; arn: string }> {
  return apiFetch('/api/policies', {
    method: 'POST',
    body: JSON.stringify({ name, document }),
  })
}

export async function getPolicy(name: string): Promise<{ name: string; arn: string; document: string }> {
  return apiFetch(`/api/policies/${encodeURIComponent(name)}`)
}

export async function deletePolicy(name: string): Promise<{ ok: boolean }> {
  return apiFetch(`/api/policies/${encodeURIComponent(name)}`, { method: 'DELETE' })
}
