import { toast as sonnerToast } from 'svelte-sonner'
import { ApiError } from '$lib/api/http'
import { isSessionExpiredError } from '$lib/api/session'

export { sonnerToast as toast }

export function toastApiError(
  err: unknown,
  fallback: string,
  options?: Parameters<typeof sonnerToast.error>[1],
) {
  if (isSessionExpiredError(err)) return
  const message =
    err instanceof ApiError ? err.message : err instanceof Error ? err.message : fallback
  sonnerToast.error(message || fallback, options)
}
