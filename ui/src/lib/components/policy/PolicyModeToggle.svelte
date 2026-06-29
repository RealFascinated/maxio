<script lang="ts">
  import { Button } from '$lib/components/ui/button'

  interface Props {
    mode: 'builder' | 'json'
    readonly?: boolean
    onchange?: (mode: 'builder' | 'json') => void
  }

  let { mode, readonly = false, onchange }: Props = $props()
</script>

<div class="inline-flex rounded-sm border-2 border-border" role="group" aria-label="Policy editor mode">
  <Button
    type="button"
    variant={mode === 'builder' ? 'highlighted' : 'ghost'}
    size="sm"
    class="rounded-none border-0"
    disabled={readonly}
    onclick={() => onchange?.('builder')}
  >
    Builder
  </Button>
  <Button
    type="button"
    variant={mode === 'json' ? 'highlighted' : 'ghost'}
    size="sm"
    class="rounded-none border-0 border-l-2 border-border"
    disabled={readonly}
    onclick={() => onchange?.('json')}
  >
    JSON
  </Button>
</div>
