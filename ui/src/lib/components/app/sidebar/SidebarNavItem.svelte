<script lang="ts">
  import type { LucideIcon } from '$lib/lucide'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { cn } from '$lib/utils'

  interface Props {
    label: string
    icon: LucideIcon
    href: string
    active?: boolean
    collapsed?: boolean
    onNavigate?: () => void
  }

  let { label, icon: Icon, href, active = false, collapsed = false, onNavigate }: Props = $props()

  const linkClass = $derived(
    cn(
      'flex h-8 w-full items-center gap-2.5 rounded-control text-sm transition-colors',
      collapsed ? 'justify-center' : 'px-2',
      active ? 'bg-selected text-foreground' : 'text-muted-foreground hover:bg-hover hover:text-foreground',
    ),
  )
</script>

{#if collapsed}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <a {...props} {href} onclick={onNavigate} class={linkClass} aria-label={label} aria-current={active ? 'page' : undefined}>
          <Icon class="size-4 shrink-0" aria-hidden="true" />
        </a>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content side="right">{label}</Tooltip.Content>
  </Tooltip.Root>
{:else}
  <a {href} onclick={onNavigate} class={linkClass} aria-current={active ? 'page' : undefined}>
    <Icon class="size-4 shrink-0" aria-hidden="true" />
    <span class="truncate">{label}</span>
  </a>
{/if}
