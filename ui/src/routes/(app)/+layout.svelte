<script lang="ts">
  import { onMount } from 'svelte'
  import { fade, fly } from 'svelte/transition'
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { afterNavigate } from '$app/navigation'
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import Login from '$lib/components/Login.svelte'
  import AppSidebar from '$lib/components/app/sidebar/AppSidebar.svelte'
  import { buildSidebarNavItems } from '$lib/components/app/sidebar/navigation'
  import {
    applyThemeToDocument,
    isThemeMode,
    nextThemeMode,
    type ThemeMode,
  } from '$lib/components/app/sidebar/theme'
  import { base } from '$app/paths'
  import Menu from 'lucide-svelte/icons/menu'
  import { Sonner } from '$lib/components/ui/sonner'
  import { checkAuth, logout, type AuthCheckResponse } from '$lib/api/auth'
  import { authKeys } from '$lib/api/keys'
  import { registerSessionExpiredHandler, setSessionActive } from '$lib/api/session'
  import { isRootOnlyPath, routes } from '$lib/navigation'
  import { queryClient } from '$lib/query/client'

  let { children } = $props()

  const authQuery = createQuery(() => ({
    queryKey: authKeys.check(),
    queryFn: checkAuth,
    retry: false,
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
  let collapsed = $state(false)
  let themeMode = $state<ThemeMode>('system')
  let isDark = $state(true)
  let mobileMenuOpen = $state(false)
  let sessionExpiredNotice = $state(false)

  function handleSessionExpired() {
    authenticatedOverride = false
    sessionIsRoot = null
    sessionExpiredNotice = true
    setSessionActive(false)
    queryClient.clear()
  }

  const isRootUser = $derived(sessionIsRoot ?? authQuery.data?.isRoot === true)

  const sidebarNavItems = $derived(
    buildSidebarNavItems({
      pathname: page.url.pathname,
      isRootUser,
    }),
  )

  function closeMobileMenu() {
    mobileMenuOpen = false
  }

  afterNavigate(() => {
    mobileMenuOpen = false
  })

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && mobileMenuOpen) {
      mobileMenuOpen = false
    }
  }

  $effect(() => {
    if (!mobileMenuOpen) return
    document.body.classList.add('overflow-hidden')
    return () => document.body.classList.remove('overflow-hidden')
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
    collapsed = localStorage.getItem('sidebar-collapsed') === 'true'
    const savedTheme = localStorage.getItem('theme')
    themeMode = isThemeMode(savedTheme) ? savedTheme : 'system'
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

  function cycleTheme() {
    applyTheme(nextThemeMode(themeMode))
  }

  function goHome() {
    goto(routes.home())
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if authQuery.isPending && authenticatedOverride === null}
  <!-- loading -->
{:else if !(authenticatedOverride ?? authQuery.isSuccess)}
  <Login onLogin={handleLogin} sessionExpired={sessionExpiredNotice} />
{:else}
  <div class="relative flex h-screen bg-background">
    {#if mobileMenuOpen}
      <button
        type="button"
        class="fixed inset-0 z-40 cursor-default bg-black/60 lg:hidden"
        aria-label="Close menu"
        onclick={closeMobileMenu}
        transition:fade={{ duration: 200 }}
      ></button>
      <div
        class="fixed inset-y-0 left-0 z-50 lg:hidden"
        transition:fly={{ x: -224, duration: 200 }}
      >
        <AppSidebar
          collapsed={false}
          variant="drawer"
          navItems={sidebarNavItems}
          username={authQuery.data?.username}
          isRoot={isRootUser}
          {themeMode}
          onToggleCollapsed={toggleSidebarCollapsed}
          onThemeChange={applyTheme}
          onCycleTheme={cycleTheme}
          onGoHome={goHome}
          onLogout={handleLogout}
        />
      </div>
    {/if}

    <AppSidebar
      {collapsed}
      variant="inline"
      class="hidden shrink-0 lg:flex"
      navItems={sidebarNavItems}
      username={authQuery.data?.username}
      isRoot={isRootUser}
      {themeMode}
      onToggleCollapsed={toggleSidebarCollapsed}
      onThemeChange={applyTheme}
      onCycleTheme={cycleTheme}
      onGoHome={goHome}
      onLogout={handleLogout}
    />

    <main class="flex min-w-0 flex-1 flex-col overflow-hidden">
      <header
        class="sticky top-0 z-30 flex h-14 shrink-0 items-center gap-3 border-b bg-background/95 px-4 backdrop-blur-sm lg:hidden"
        style="border-color: var(--cool-sidebar-border);"
      >
        <button
          type="button"
          onclick={() => (mobileMenuOpen = true)}
          class="-m-2.5 rounded-sm p-2.5 text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-warning dark:hover:text-warning dark:focus-visible:ring-warning"
          aria-label="Open menu"
          aria-expanded={mobileMenuOpen}
        >
          <Menu class="size-5" />
        </button>
        <a
          href={routes.home()}
          class="flex items-center gap-2 rounded-sm transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:hover:text-warning dark:focus-visible:ring-warning"
          aria-label="Go to buckets"
          title="Buckets"
        >
          <img src={`${base}/maxio.png`} alt="" class="size-6 shrink-0" aria-hidden="true" />
          <span class="text-xl font-bold tracking-tight text-foreground">MaxIO</span>
        </a>
      </header>

      <div class="flex-1 overflow-auto p-6">
        {@render children()}
      </div>
    </main>
  </div>
  <Sonner theme={isDark ? 'dark' : 'light'} />
{/if}
