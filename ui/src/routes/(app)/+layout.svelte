<script lang="ts">
  import { onMount } from 'svelte'
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { afterNavigate } from '$app/navigation'
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import Login from '$lib/components/Login.svelte'
  import AppSidebar from '$lib/components/app/sidebar/AppSidebar.svelte'
  import MobileNavSheet from '$lib/components/app/sidebar/MobileNavSheet.svelte'
  import TopBar from '$lib/components/app/topbar/TopBar.svelte'
  import AppCommands from '$lib/components/app/commands/AppCommands.svelte'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { appUi } from '$lib/app-ui.svelte'
  import { buildSidebarNavItems } from '$lib/components/app/sidebar/navigation'
  import {
    applyThemeToDocument,
    isThemeMode,
    type ThemeMode,
  } from '$lib/components/app/sidebar/theme'
  import { Sonner } from '$lib/components/ui/sonner'
  import { checkAuth, isNotAuthenticatedError, logout, type AuthCheckResponse } from '$lib/api/auth'
  import { authKeys } from '$lib/api/keys'
  import { registerSessionExpiredHandler, setSessionActive } from '$lib/api/session'
  import { isRootOnlyPath, routes } from '$lib/navigation'
  import { queryClient } from '$lib/query/client'

  let { children } = $props()

  const authQuery = createQuery(() => ({
    queryKey: authKeys.check(),
    queryFn: checkAuth,
    retry: false,
    refetchInterval: (query) =>
      query.state.error && !isNotAuthenticatedError(query.state.error) ? 2_000 : false,
  }))
  const logoutMutation = createMutation(() => ({
    mutationFn: logout,
    onSuccess: () => {
      queryClient.clear()
      queryClient.invalidateQueries({ queryKey: authKeys.all })
    },
  }))

  let authenticatedOverride = $state<boolean | null>(null)
  let sessionIsRoot = $state<boolean | null>(null)
  let collapsed = $state(true)
  let themeMode = $state<ThemeMode>('dark')
  let isDark = $state(true)
  let sessionExpiredNotice = $state(false)

  function handleSessionExpired() {
    authenticatedOverride = false
    sessionIsRoot = null
    sessionExpiredNotice = true
    setSessionActive(false)
    queryClient.clear()
  }

  const authUnavailable = $derived(authQuery.isError && !isNotAuthenticatedError(authQuery.error))
  const sessionKnown = $derived(
    authenticatedOverride ?? (authUnavailable ? authQuery.data?.ok === true : authQuery.isSuccess),
  )

  const isRootUser = $derived(sessionIsRoot ?? authQuery.data?.isRoot === true)

  const sidebarNavItems = $derived(
    buildSidebarNavItems({
      pathname: page.url.pathname,
      isRootUser,
    }),
  )

  afterNavigate(() => {
    appUi.mobileNavOpen = false
  })

  function toggleSidebarCollapsed() {
    collapsed = !collapsed
    localStorage.setItem('sidebar-collapsed', String(collapsed))
  }

  $effect(() => {
    if (authenticatedOverride === true || authQuery.data?.ok === true) {
      setSessionActive(true)
    }
  })

  $effect(() => {
    if (authQuery.data?.isRoot !== undefined) {
      sessionIsRoot = authQuery.data.isRoot === true
    }
  })

  $effect(() => {
    const authResolved =
      authenticatedOverride !== null || authQuery.isSuccess || authQuery.isError
    if (!authResolved) return
    if (isRootOnlyPath(page.url.pathname) && !isRootUser) {
      goto(routes.home(), { replaceState: true })
    }
  })

  onMount(() => {
    collapsed = localStorage.getItem('sidebar-collapsed') !== 'false'
    const savedTheme = localStorage.getItem('theme')
    themeMode = isThemeMode(savedTheme) ? savedTheme : 'dark'
    applyTheme(themeMode, false)

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemThemeChange = () => {
      if (themeMode === 'system') {
        applyTheme('system', false)
      }
    }
    mediaQuery.addEventListener('change', handleSystemThemeChange)

    registerSessionExpiredHandler(handleSessionExpired)

    return () => {
      mediaQuery.removeEventListener('change', handleSystemThemeChange)
    }
  })

  function handleLogin(session: AuthCheckResponse) {
    authenticatedOverride = true
    sessionIsRoot = session.isRoot === true
    sessionExpiredNotice = false
    setSessionActive(true)
    queryClient.setQueryData(authKeys.check(), session)
    queryClient.invalidateQueries({ queryKey: authKeys.all })
  }

  async function handleLogout() {
    setSessionActive(false)
    await logoutMutation.mutateAsync()
    authenticatedOverride = false
    sessionIsRoot = null
    goto(routes.home())
  }

  function applyTheme(mode: ThemeMode, persist = true) {
    themeMode = mode
    isDark = applyThemeToDocument(mode)
    if (persist) {
      localStorage.setItem('theme', mode)
    }
  }

  function goHome() {
    goto(routes.home())
  }
</script>

{#if authQuery.isPending && authenticatedOverride === null}
  <!-- loading -->
{:else if authUnavailable && !sessionKnown}
  <div class="flex min-h-screen w-full items-center justify-center bg-background px-6 py-8">
    <p class="text-center text-sm text-muted-foreground">Server unavailable. Retrying…</p>
  </div>
{:else if !sessionKnown}
  <Login onLogin={handleLogin} sessionExpired={sessionExpiredNotice} />
{:else}
  <Tooltip.Provider delayDuration={300}>
    <div class="flex h-screen bg-background">
      <AppSidebar
        {collapsed}
        class="hidden lg:flex"
        navItems={sidebarNavItems}
        onToggleCollapsed={toggleSidebarCollapsed}
        onGoHome={goHome}
      />

      <div class="flex min-w-0 flex-1 flex-col">
        <TopBar
          pathname={page.url.pathname}
          username={authQuery.data?.username}
          isRoot={isRootUser}
          {themeMode}
          {isDark}
          onThemeChange={applyTheme}
          onLogout={handleLogout}
        />
        <main class="min-h-0 flex-1 overflow-auto p-4">
          {@render children()}
        </main>
      </div>
    </div>

    <MobileNavSheet bind:open={appUi.mobileNavOpen} navItems={sidebarNavItems} />
    <AppCommands
      isRoot={isRootUser}
      onLogout={handleLogout}
      onToggleTheme={() => applyTheme(isDark ? 'light' : 'dark')}
      onToggleSidebar={toggleSidebarCollapsed}
    />
  </Tooltip.Provider>
  <Sonner theme={isDark ? 'dark' : 'light'} />
{/if}
