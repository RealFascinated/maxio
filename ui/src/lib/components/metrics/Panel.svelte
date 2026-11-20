<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils'

  interface Props {
    title: string
    actions?: Snippet
    children: Snippet
    class?: string
    bodyClass?: string
  }

  let { title, actions, children, class: className, bodyClass }: Props = $props()
</script>

<section class={cn('min-w-0 rounded-surface border border-border bg-panel', className)}>
  <header class="flex min-h-9 flex-wrap items-center justify-between gap-2 border-b border-border px-3 py-1">
    <h2 class="text-sm">{title}</h2>
    {@render actions?.()}
  </header>
  <div class={cn('p-3', bodyClass)}>
    {@render children()}
  </div>
</section>
