<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
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

  let { open = $bindable(false), title, description, onClose, children, footer }: Props = $props()

  function handleOpenChange(next: boolean) {
    if (next) return
    if (onClose) onClose()
    else open = false
  }
</script>

<DialogPrimitive.Root {open} onOpenChange={handleOpenChange}>
  <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay
      class="fixed inset-0 z-50 bg-black/40 data-[state=open]:animate-[ui-fade-in_120ms_ease-out]"
    />
    <DialogPrimitive.Content
      class="fixed inset-y-0 right-0 z-50 flex w-full max-w-md flex-col border-l border-border-strong bg-panel shadow-pop outline-none data-[state=open]:animate-[ui-slide-in-right_160ms_ease-out]"
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
            <Button {...props} variant="ghost" size="icon-xs" aria-label="Close panel"><X /></Button>
          {/snippet}
        </DialogPrimitive.Close>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto p-4 text-sm">
        {@render children?.()}
      </div>

      {#if footer}
        <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border bg-inset px-4 py-2.5">
          {@render footer()}
        </div>
      {/if}
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
