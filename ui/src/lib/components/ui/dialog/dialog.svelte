<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import { Button } from '$lib/components/ui/button'
  import X from 'lucide-svelte/icons/x'
  import { cn } from '$lib/utils'

  type Props = {
    open?: boolean
    title: string
    description?: string
    loading?: boolean
    size?: 'default' | 'lg'
    onClose?: () => void
    children?: import('svelte').Snippet
    footer?: import('svelte').Snippet
  }

  let {
    open = $bindable(false),
    title,
    description,
    loading = false,
    size = 'default',
    onClose,
    children,
    footer,
  }: Props = $props()

  function handleOpenChange(next: boolean) {
    if (next || loading) return
    if (onClose) onClose()
    else open = false
  }
</script>

<DialogPrimitive.Root {open} onOpenChange={handleOpenChange}>
  <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay
      class="fixed inset-0 z-50 bg-black/50 data-[state=open]:animate-[ui-fade-in_120ms_ease-out]"
    />
    <DialogPrimitive.Content
      interactOutsideBehavior={loading ? 'ignore' : 'close'}
      escapeKeydownBehavior={loading ? 'ignore' : 'close'}
      class={cn(
        'fixed left-1/2 top-1/2 z-50 flex max-h-[calc(100vh-2rem)] w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-surface border border-border-strong bg-panel shadow-pop outline-none data-[state=open]:animate-[ui-pop-in_120ms_ease-out]',
        size === 'lg' ? 'max-w-4xl' : 'max-w-lg',
      )}
    >
      <div class="flex shrink-0 items-center justify-between gap-3 border-b border-border px-4 py-2.5">
        <div class="flex min-w-0 items-baseline gap-2">
          <DialogPrimitive.Title class="truncate text-sm font-semibold">{title}</DialogPrimitive.Title>
          {#if description}
            <DialogPrimitive.Description class="truncate text-xs text-muted-foreground">
              {description}
            </DialogPrimitive.Description>
          {/if}
        </div>
        <DialogPrimitive.Close>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-xs" aria-label="Close dialog" disabled={loading}>
              <X />
            </Button>
          {/snippet}
        </DialogPrimitive.Close>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-4 text-sm">
        {@render children?.()}
      </div>

      <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border bg-inset px-4 py-2.5">
        {#if footer}
          {@render footer()}
        {:else}
          <Button variant="secondary" disabled={loading} onclick={() => handleOpenChange(false)}>Close</Button>
        {/if}
      </div>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
