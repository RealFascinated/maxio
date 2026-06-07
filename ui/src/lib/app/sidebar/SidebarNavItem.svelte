<script lang="ts">
  import type { Component } from 'svelte'

  interface Props {
    label: string
    icon: Component
    active?: boolean
    collapsed?: boolean
    onSelect: () => void
  }

  let {
    label,
    icon: Icon,
    active = false,
    collapsed = false,
    onSelect,
  }: Props = $props()
</script>

<button
  type="button"
  onclick={onSelect}
  class="flex min-h-7 w-full items-center rounded-sm py-1 text-left text-sm font-medium transition-colors overflow-hidden"
  class:gap-3={!collapsed}
  class:px-2={!collapsed}
  class:justify-center={collapsed}
  class:size-8={collapsed}
  class:bg-neutral-200={active}
  class:text-black={active}
  class:dark:bg-coolgray-200={active}
  class:dark:text-warning={active}
  class:hover:bg-neutral-300={active}
  class:dark:hover:bg-coolgray-100={active}
  class:text-muted-foreground={!active}
  class:hover:bg-muted={!active}
  aria-current={active ? 'page' : undefined}
  title={label}
>
  <Icon class="size-4 shrink-0" aria-hidden="true" />
  {#if !collapsed}
    <span class="whitespace-nowrap">{label}</span>
  {/if}
</button>
