<script lang="ts">
  import { base } from '$lib/base'
  import PanelLeft from 'lucide-svelte/icons/panel-left'
  import SidebarNavItem from '$lib/components/app/sidebar/SidebarNavItem.svelte'
  import type { SidebarNavEntry } from '$lib/components/app/sidebar/navigation'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { cn } from '$lib/utils'

  interface Props {
    collapsed: boolean
    navItems: SidebarNavEntry[]
    class?: string
    onToggleCollapsed: () => void
    onGoHome: () => void
  }

  let { collapsed, navItems, class: className = '', onToggleCollapsed, onGoHome }: Props = $props()

  const mainItems = $derived(navItems.filter((item) => item.placement === 'main'))
  const footerItems = $derived(navItems.filter((item) => item.placement === 'footer'))
</script>

<nav
  class={cn(
    'flex shrink-0 flex-col border-r border-border bg-panel',
    collapsed ? 'w-12' : 'w-52',
    className,
  )}
  aria-label="Main navigation"
>
  <div class={cn('flex h-10 shrink-0 items-center border-b border-border', collapsed ? 'justify-center' : 'px-3')}>
    <button
      type="button"
      onclick={onGoHome}
      class="flex min-w-0 items-center gap-2 rounded-control"
      aria-label="MaxIO, go to buckets"
    >
      <img src={`${base}/maxio.png`} alt="" class="size-5 shrink-0" aria-hidden="true" />
      {#if !collapsed}<span class="text-sm font-semibold tracking-tight">MaxIO</span>{/if}
    </button>
  </div>

  <div class="flex flex-1 flex-col gap-1 overflow-y-auto p-2" role="list">
    {#each mainItems as item (item.id)}
      <SidebarNavItem label={item.label} icon={item.icon} href={item.href} active={item.active} {collapsed} />
    {/each}
  </div>

  <div class="flex shrink-0 flex-col gap-1 border-t border-border p-2">
    {#each footerItems as item (item.id)}
      <SidebarNavItem label={item.label} icon={item.icon} href={item.href} active={item.active} {collapsed} />
    {/each}
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <button
            {...props}
            type="button"
            onclick={onToggleCollapsed}
            class={cn(
              'flex h-8 w-full items-center gap-2.5 rounded-control text-sm text-muted-foreground transition-colors hover:bg-hover hover:text-foreground',
              collapsed ? 'justify-center' : 'px-2',
            )}
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            aria-expanded={!collapsed}
          >
            <PanelLeft class="size-4 shrink-0" aria-hidden="true" />
            {#if !collapsed}<span class="truncate text-left">Collapse</span>{/if}
          </button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content side="right">{collapsed ? 'Expand' : 'Collapse'} sidebar [</Tooltip.Content>
    </Tooltip.Root>
  </div>
</nav>
