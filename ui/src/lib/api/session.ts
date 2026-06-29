import { ApiError } from './errors'

const PUBLIC_AUTH_PATHS = ['/api/auth/check', '/api/auth/login', '/api/auth/config'] as const

export class SessionExpiredError extends ApiError {
  constructor(payload?: unknown) {
    super('Session expired', 401, payload)
    this.name = 'SessionExpiredError'
  }
}

export function isSessionExpiredError(err: unknown): err is SessionExpiredError {
  return err instanceof SessionExpiredError
}

function requestPath(url: string): string {
  const path = url.startsWith('http') ? new URL(url).pathname : url
  return path.split('?')[0] ?? path
}

function isPublicAuthRequest(url: string): boolean {
  const path = requestPath(url)
  return PUBLIC_AUTH_PATHS.some((p) => path === p || path.endsWith(p))
}

let sessionActive = false
let expiryInProgress = false
let handler: (() => void) | null = null

export function setSessionActive(active: boolean) {
  sessionActive = active
  if (active) {
    expiryInProgress = false
  }
}

export function registerSessionExpiredHandler(fn: () => void) {
  handler = fn
}

/** Returns true when the 401 was handled as an expired session. */
export function notifyUnauthorized(url: string): boolean {
  if (isPublicAuthRequest(url)) return false
  if (expiryInProgress) return true
  if (!sessionActive) return false

  expiryInProgress = true
  sessionActive = false
  handler?.()
  return true
}
