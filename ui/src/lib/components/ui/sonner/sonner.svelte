<script lang="ts">
  import { Toaster } from 'svelte-sonner'

  interface Props {
    theme?: 'light' | 'dark'
  }
  let { theme = 'dark' }: Props = $props()
</script>

<Toaster
  {theme}
  position="bottom-right"
  expand
  visibleToasts={5}
  closeButton
  duration={4000}
  toastOptions={{
    classes: {
      toast:
        'group !rounded-surface !font-sans !text-sm !bg-raised !text-foreground !border !border-border-strong !shadow-none !py-2 !px-3',
      title: '!font-medium',
      description: '!text-muted-foreground',
      success: '[&_[data-icon]]:!text-success',
      error: '[&_[data-icon]]:!text-danger',
      warning: '[&_[data-icon]]:!text-warning',
      info: '[&_[data-icon]]:!text-info',
      closeButton: '!bg-raised !border-border-strong !text-muted-foreground hover:!text-foreground',
    },
  }}
/>
