<script lang="ts">
  import { base } from '$app/paths'
  import LogOut from 'lucide-svelte/icons/log-out'
  import SidebarNavItem from '$lib/app/sidebar/SidebarNavItem.svelte'
  import SidebarThemeControl from '$lib/app/sidebar/SidebarThemeControl.svelte'
  import type { SidebarNavEntry } from '$lib/app/sidebar/navigation'
  import type { ThemeMode } from '$lib/app/sidebar/theme'

  interface Props {
    collapsed: boolean
    navItems: SidebarNavEntry[]
    themeMode: ThemeMode
    variant?: 'inline' | 'drawer'
    class?: string
    onToggleCollapsed: () => void
    onThemeChange: (mode: ThemeMode) => void
    onCycleTheme: () => void
    onLogout: () => void
  }

  let {
    collapsed,
    navItems,
    themeMode,
    variant = 'inline',
    class: className = '',
    onToggleCollapsed,
    onThemeChange,
    onCycleTheme,
    onLogout,
  }: Props = $props()

  const isDrawer = $derived(variant === 'drawer')
  const isCollapsed = $derived(isDrawer ? false : collapsed)
</script>

<nav
  class="relative flex flex-col border-r bg-sidebar-background transition-[width] duration-200 {className}"
  class:w-56={isDrawer}
  class:w-64={!isDrawer && !isCollapsed}
  class:w-16={!isDrawer && isCollapsed}
  class:h-full={isDrawer}
  style="border-color: var(--cool-sidebar-border);"
  aria-label="Main navigation"
>
  {#if !isDrawer}
    <button
      type="button"
      onclick={onToggleCollapsed}
      class="absolute top-8 -right-3 z-10 hidden size-6 items-center justify-center rounded-full border bg-card text-muted-foreground shadow-sm transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:focus-visible:ring-warning focus-visible:ring-offset-2 dark:focus-visible:ring-offset-base lg:flex"
      style="border-color: var(--cool-sidebar-border);"
      title={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-label={isCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-expanded={!isCollapsed}
    >
      <svg
        class="size-3.5 transition-transform"
        class:rotate-180={isCollapsed}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M15 18 9 12l6-6" />
      </svg>
    </button>
  {/if}

  <header
    class="flex h-14 shrink-0 items-center overflow-hidden"
    class:px-4={!isCollapsed}
    class:justify-center={isCollapsed}
  >
    <img src={`${base}/maxio.png`} alt="" class="size-6 shrink-0" aria-hidden="true" />
    {#if !isCollapsed}
      <span class="ml-2 text-2xl font-bold tracking-tight text-foreground whitespace-nowrap">
        MaxIO
      </span>
    {/if}
  </header>

  <div class="flex flex-1 flex-col gap-0.5 overflow-y-auto p-2" role="list">
    {#each navItems as item (item.id)}
      <SidebarNavItem
        label={item.label}
        icon={item.icon}
        active={item.active}
        collapsed={isCollapsed}
        onSelect={item.onSelect}
      />
    {/each}
  </div>

  <footer class="flex shrink-0 flex-col gap-0.5 border-t p-2" style="border-color: var(--cool-sidebar-border);">
    <SidebarThemeControl
      collapsed={isCollapsed}
      {themeMode}
      onThemeChange={onThemeChange}
      onCycleTheme={onCycleTheme}
    />
    <button
      type="button"
      onclick={onLogout}
      class="flex min-h-7 w-full items-center rounded-sm py-1 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-muted overflow-hidden"
      class:gap-3={!isCollapsed}
      class:px-2={!isCollapsed}
      class:justify-center={isCollapsed}
      class:size-8={isCollapsed}
      aria-label="Sign out"
      title="Sign out"
    >
      <LogOut class="size-4 shrink-0" aria-hidden="true" />
      {#if !isCollapsed}
        <span class="whitespace-nowrap">Sign out</span>
      {/if}
    </button>
  </footer>
</nav>
