<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Callout } from "$lib/components/ui/callout";
  import { createMutation, createQuery } from "@tanstack/svelte-query";
  import Eye from "lucide-svelte/icons/eye";
  import EyeOff from "lucide-svelte/icons/eye-off";
  import {
    browserCanStoreSecureCookies,
    fetchAuthConfig,
    login,
    type AuthCheckResponse,
  } from "$lib/api/auth";
  import { authKeys } from "$lib/api/keys";
  import { ApiError } from "$lib/api/http";

  let accessKey = $state('')
  let secretKey = $state('')
  let error = $state('')
  let accessKeyError = $state('')
  let secretKeyError = $state('')
  let showSecret = $state(false)

  const authConfigQuery = createQuery(() => ({
    queryKey: authKeys.config(),
    queryFn: fetchAuthConfig,
    staleTime: Infinity,
    refetchInterval: (query) => (query.state.error ? 2_000 : false),
  }))

  const loginBlocked = $derived(
    authConfigQuery.data?.cookiesRequireHttps === true && !browserCanStoreSecureCookies(),
  )

  const loginMutation = createMutation(() => ({
    mutationFn: login,
    onSuccess: (data) => onLogin(data),
  }))

  interface Props {
    onLogin: (session: AuthCheckResponse) => void
    sessionExpired?: boolean
  }
  let { onLogin, sessionExpired = false }: Props = $props()

  async function handleSubmit(e: Event) {
    e.preventDefault()
    if (loginBlocked) return
    error = ''
    accessKeyError = accessKey.trim() ? '' : 'Access key is required'
    secretKeyError = secretKey.trim() ? '' : 'Secret key is required'
    if (accessKeyError || secretKeyError) return
    try {
      await loginMutation.mutateAsync({ accessKey, secretKey })
    } catch (err) {
      console.error('Login failed:', err)
      error = err instanceof ApiError ? err.message : 'Connection failed'
    }
  }
</script>

<div class="flex min-h-screen w-full items-center justify-center bg-background p-4">
  <div class="w-full max-w-sm rounded-surface border border-border bg-panel p-4">
    <h1 class="mb-4">MaxIO</h1>

    <form onsubmit={handleSubmit} class="flex flex-col gap-3">
      <div class="flex flex-col gap-1.5">
        <Label for="accessKey">Access key</Label>
        <Input
          id="accessKey"
          type="text"
          bind:value={accessKey}
          autocomplete="username"
          disabled={loginBlocked}
          aria-invalid={accessKeyError ? true : undefined}
          aria-describedby={accessKeyError ? 'accessKey-error' : undefined}
        />
        {#if accessKeyError}
          <p id="accessKey-error" class="text-xs text-danger">{accessKeyError}</p>
        {/if}
      </div>

      <div class="flex flex-col gap-1.5">
        <Label for="secretKey">Secret key</Label>
        <div class="relative">
          <Input
            id="secretKey"
            type={showSecret ? 'text' : 'password'}
            bind:value={secretKey}
            autocomplete="current-password"
            class="pr-8"
            disabled={loginBlocked}
            aria-invalid={secretKeyError ? true : undefined}
            aria-describedby={secretKeyError ? 'secretKey-error' : undefined}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            class="absolute right-0.5 top-0.5"
            aria-label={showSecret ? 'Hide secret key' : 'Show secret key'}
            aria-pressed={showSecret}
            onclick={() => (showSecret = !showSecret)}
          >
            {#if showSecret}
              <EyeOff />
            {:else}
              <Eye />
            {/if}
          </Button>
        </div>
        {#if secretKeyError}
          <p id="secretKey-error" class="text-xs text-danger">{secretKeyError}</p>
        {/if}
      </div>

      {#if authConfigQuery.isError}
        <Callout type="warning">Server unavailable. Retrying…</Callout>
      {/if}

      {#if loginBlocked}
        <Callout type="warning" title="HTTPS required">
          Session cookies are Secure. Use HTTPS or set
          <code class="font-mono text-xs">MAXIO_ALLOW_INSECURE_DEV=true</code> or
          <code class="font-mono text-xs">MAXIO_SECURE_COOKIES=false</code>.
        </Callout>
      {/if}

      {#if sessionExpired}
        <Callout type="warning">Session expired. Sign in again.</Callout>
      {/if}

      {#if error}
        <Callout type="danger">{error}</Callout>
      {/if}

      <Button
        type="submit"
        size="lg"
        class="mt-1 w-full"
        loading={loginMutation.isPending}
        disabled={loginBlocked}
      >
        Sign in
      </Button>
    </form>
  </div>
</div>
