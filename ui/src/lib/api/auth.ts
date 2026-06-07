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

export interface AuthConfigResponse {
  cookiesRequireHttps: boolean
}

export interface LoginInput { accessKey: string; secretKey: string }

/** Browsers accept Secure cookies over HTTP only on loopback hosts. */
export function browserCanStoreSecureCookies(): boolean {
  if (window.location.protocol === 'https:') return true
  const host = window.location.hostname
  return host === 'localhost' || host === '127.0.0.1' || host === '[::1]'
}

export async function fetchAuthConfig(): Promise<AuthConfigResponse> {
  return apiFetch<AuthConfigResponse>('/api/auth/config')
}

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
