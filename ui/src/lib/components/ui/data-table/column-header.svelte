<script lang="ts">
  import ArrowDown from 'lucide-svelte/icons/arrow-down'
  import ArrowUp from 'lucide-svelte/icons/arrow-up'
  import ChevronsUpDown from 'lucide-svelte/icons/chevrons-up-down'
  import { cn } from '$lib/utils'

  interface Props {
    header: {
      column: {
        getCanSort: () => boolean
        getIsSorted: () => false | 'asc' | 'desc'
        getToggleSortingHandler: () => ((event: unknown) => void) | undefined
      }
    }
    title: string
    class?: string
  }

  let { header, title, class: className }: Props = $props()

  const sorted = $derived(header.column.getIsSorted())
</script>

{#if header.column.getCanSort()}
  <button
    type="button"
    class={cn(
      '-mx-1 inline-flex h-6 items-center gap-1 rounded-control px-1 transition-colors hover:bg-hover hover:text-foreground',
      className,
    )}
    onclick={header.column.getToggleSortingHandler()}
  >
    <span>{title}</span>
    {#if sorted === 'desc'}
      <ArrowDown class="size-3 text-foreground" />
    {:else if sorted === 'asc'}
      <ArrowUp class="size-3 text-foreground" />
    {:else}
      <ChevronsUpDown class="size-3 opacity-40" />
    {/if}
  </button>
{:else}
  <span class={className}>{title}</span>
{/if}
