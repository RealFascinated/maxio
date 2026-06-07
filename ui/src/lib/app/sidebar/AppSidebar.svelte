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
    onToggleCollapsed: () => void
    onThemeChange: (mode: ThemeMode) => void
    onCycleTheme: () => void
    onLogout: () => void
  }

  let {
    collapsed,
    navItems,
    themeMode,
    onToggleCollapsed,
    onThemeChange,
    onCycleTheme,
    onLogout,
  }: Props = $props()
</script>

<nav
  class="relative flex flex-col border-r bg-sidebar-background transition-[width] duration-200"
  class:w-64={!collapsed}
  class:w-16={collapsed}
  style="border-color: var(--cool-sidebar-border);"
  aria-label="Main navigation"
>
  <button
    type="button"
    onclick={onToggleCollapsed}
    class="absolute top-8 -right-3 z-10 flex size-6 items-center justify-center rounded-full border bg-card text-muted-foreground shadow-sm transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:focus-visible:ring-warning focus-visible:ring-offset-2 dark:focus-visible:ring-offset-base"
    style="border-color: var(--cool-sidebar-border);"
    title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    aria-expanded={!collapsed}
  >
    <svg
      class="size-3.5 transition-transform"
      class:rotate-180={collapsed}
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

  <header
    class="flex h-14 shrink-0 items-center overflow-hidden"
    class:px-4={!collapsed}
    class:justify-center={collapsed}
  >
    <img src={`${base}/maxio.png`} alt="" class="size-6 shrink-0" aria-hidden="true" />
    {#if !collapsed}
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
        {collapsed}
        onSelect={item.onSelect}
      />
    {/each}
  </div>

  <footer class="flex shrink-0 flex-col gap-0.5 border-t p-2" style="border-color: var(--cool-sidebar-border);">
    <SidebarThemeControl
      {collapsed}
      {themeMode}
      onThemeChange={onThemeChange}
      onCycleTheme={onCycleTheme}
    />
    <button
      type="button"
      onclick={onLogout}
      class="flex min-h-7 w-full items-center rounded-sm py-1 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-muted overflow-hidden"
      class:gap-3={!collapsed}
      class:px-2={!collapsed}
      class:justify-center={collapsed}
      class:size-8={collapsed}
      aria-label="Sign out"
      title="Sign out"
    >
      <LogOut class="size-4 shrink-0" aria-hidden="true" />
      {#if !collapsed}
        <span class="whitespace-nowrap">Sign out</span>
      {/if}
    </button>
  </footer>
</nav>
