import { apiFetch } from './http'

export interface AuthCapabilities {
  canCreateBucket: boolean
  canListAllBuckets: boolean
  canManageUsers: boolean
}

export interface AuthCheckResponse {
  ok: boolean
  username?: string
  isRoot?: boolean
  capabilities?: AuthCapabilities
}
export interface LoginInput { accessKey: string; secretKey: string }

export async function checkAuth(): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/check')
}

export async function login(input: LoginInput): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function logout(): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/logout', { method: 'POST' })
}
