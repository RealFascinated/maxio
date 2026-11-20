<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import { Button, type ButtonVariant } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import X from 'lucide-svelte/icons/x'

  type Props = {
    open?: boolean
    title: string
    description?: string
    confirmLabel?: string
    cancelLabel?: string
    confirmVariant?: ButtonVariant
    confirmationText?: string
    confirmationLabel?: string
    loading?: boolean
    onClose?: () => void
    onConfirm: () => void | Promise<void>
  }

  let {
    open = $bindable(false),
    title,
    description,
    confirmLabel = 'Confirm',
    cancelLabel = 'Cancel',
    confirmVariant = 'default',
    confirmationText,
    confirmationLabel,
    loading = false,
    onClose,
    onConfirm,
  }: Props = $props()

  let typedConfirmation = $state('')
  let confirmationInput = $state<HTMLInputElement | null>(null)
  const canConfirm = $derived(!confirmationText || typedConfirmation === confirmationText)

  $effect(() => {
    if (!open) typedConfirmation = ''
  })

  function close() {
    if (loading) return
    if (onClose) onClose()
    else open = false
  }

  async function confirm() {
    if (!canConfirm || loading) return
    await onConfirm()
  }
</script>

<DialogPrimitive.Root {open} onOpenChange={(next) => !next && close()}>
  <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay
      class="fixed inset-0 z-50 bg-black/50 data-[state=open]:animate-[ui-fade-in_120ms_ease-out]"
    />
    <DialogPrimitive.Content
      interactOutsideBehavior={loading ? 'ignore' : 'close'}
      escapeKeydownBehavior={loading ? 'ignore' : 'close'}
      onOpenAutoFocus={(event) => {
        if (!confirmationText) return
        event.preventDefault()
        confirmationInput?.focus()
      }}
      class="fixed left-1/2 top-1/2 z-50 w-[calc(100vw-2rem)] max-w-md -translate-x-1/2 -translate-y-1/2 rounded-surface border border-border-strong bg-panel shadow-pop outline-none data-[state=open]:animate-[ui-pop-in_120ms_ease-out]"
    >
      <div class="flex items-start justify-between gap-3 px-4 pt-3">
        <div class="flex min-w-0 flex-col gap-1">
          <DialogPrimitive.Title class="text-sm font-semibold">{title}</DialogPrimitive.Title>
          {#if description}
            <DialogPrimitive.Description class="text-sm text-muted-foreground">
              {description}
            </DialogPrimitive.Description>
          {/if}
        </div>
        <Button variant="ghost" size="icon-xs" aria-label="Close confirmation" disabled={loading} onclick={close}>
          <X />
        </Button>
      </div>

      {#if confirmationText}
        <label class="mt-3 flex flex-col gap-1.5 px-4 text-xs text-muted-foreground">
          <span>
            Type <span class="font-mono font-medium text-foreground">{confirmationText}</span> to confirm
            {#if confirmationLabel}({confirmationLabel}){/if}
          </span>
          <Input
            bind:ref={confirmationInput}
            bind:value={typedConfirmation}
            autocomplete="off"
            disabled={loading}
            onkeydown={(event) => event.key === 'Enter' && confirm()}
          />
        </label>
      {/if}

      <div class="mt-4 flex flex-wrap justify-end gap-2 border-t border-border bg-inset px-4 py-2.5">
        <Button variant="secondary" disabled={loading} onclick={close}>{cancelLabel}</Button>
        <Button variant={confirmVariant} loading={loading} disabled={!canConfirm} onclick={confirm}>
          {confirmLabel}
        </Button>
      </div>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
