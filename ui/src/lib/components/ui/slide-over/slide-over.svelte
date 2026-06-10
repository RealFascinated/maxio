<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import X from 'lucide-svelte/icons/x'

  type Props = {
    open?: boolean
    title: string
    description?: string
    onClose?: () => void
    children?: import('svelte').Snippet
    footer?: import('svelte').Snippet
  }

  let {
    open = $bindable(false),
    title,
    description,
    onClose,
    children,
    footer,
  }: Props = $props()

  function close() {
    if (onClose) onClose()
    else open = false
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open) return
    if (event.key === 'Escape') {
      event.preventDefault()
      close()
    }
  }

  $effect(() => {
    if (!open) return
    document.body.classList.add('overflow-hidden')
    return () => document.body.classList.remove('overflow-hidden')
  })
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <button
    type="button"
    class="fixed inset-0 z-40 cursor-default bg-black/60"
    aria-label="Close panel"
    onclick={close}
  ></button>
  <div class="fixed inset-y-0 right-0 z-50 flex max-w-full pl-10">
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="slide-over-title"
      tabindex="-1"
      class="flex h-full w-screen max-w-xl flex-col border-l border-neutral-200 bg-neutral-50 shadow-lg dark:border-coolgray-200 dark:bg-base"
    >
      <div class="flex shrink-0 items-start justify-between gap-4 border-b border-neutral-200 p-6 dark:border-coolgray-200">
        <div class="min-w-0 flex-1">
          <h2 id="slide-over-title" class="truncate text-base font-bold text-black dark:text-white">{title}</h2>
          {#if description}
            <p class="mt-1 truncate text-sm text-neutral-600 dark:text-neutral-400">{description}</p>
          {/if}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          class="shrink-0"
          aria-label="Close panel"
          onclick={close}
        >
          <X class="size-5" />
        </Button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-6 text-sm text-neutral-700 dark:text-neutral-300">
        {@render children?.()}
      </div>

      {#if footer}
        <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t border-neutral-200 p-6 dark:border-coolgray-200">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}
