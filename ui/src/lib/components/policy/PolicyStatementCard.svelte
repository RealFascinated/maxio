<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Label } from '$lib/components/ui/label'
  import PolicyActionField from './PolicyActionField.svelte'
  import PolicyPrincipalField from './PolicyPrincipalField.svelte'
  import PolicyResourceField from './PolicyResourceField.svelte'
  import type { PolicyStatement, PolicyVariant } from '$lib/policy/types'

  interface Props {
    statement: PolicyStatement
    index: number
    total: number
    variant: PolicyVariant
    accessibleBuckets?: string[]
    bucketsLoading?: boolean
    readonly?: boolean
    onchange?: (statement: PolicyStatement) => void
    ondelete?: () => void
    onmove?: (direction: -1 | 1) => void
  }

  let {
    statement,
    index,
    total,
    variant,
    accessibleBuckets = [],
    bucketsLoading = false,
    readonly = false,
    onchange,
    ondelete,
    onmove,
  }: Props = $props()

  function emit() {
    onchange?.({ ...statement })
  }

  function setEffect(effect: 'Allow' | 'Deny') {
    if (readonly) return
    onchange?.({ ...statement, Effect: effect })
  }
</script>

<div class="space-y-4 rounded-sm border-2 border-border p-3">
  {#if !readonly && total > 1}
    <div class="flex items-center justify-end gap-1">
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        disabled={index === 0}
        aria-label="Move statement up"
        onclick={() => onmove?.(-1)}
      >
        ↑
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        disabled={index >= total - 1}
        aria-label="Move statement down"
        onclick={() => onmove?.(1)}
      >
        ↓
      </Button>
      <Button type="button" variant="ghost" size="icon-sm" aria-label="Delete statement" onclick={() => ondelete?.()}>
        ×
      </Button>
    </div>
  {/if}

  <div class="space-y-1.5">
    <Label>Effect</Label>
    <div class="flex gap-2">
      <Button
        type="button"
        variant={statement.Effect === 'Allow' ? 'highlighted' : 'outline'}
        size="sm"
        disabled={readonly}
        onclick={() => setEffect('Allow')}
      >
        Allow
      </Button>
      <Button
        type="button"
        variant={statement.Effect === 'Deny' ? 'destructive' : 'outline'}
        size="sm"
        disabled={readonly}
        onclick={() => setEffect('Deny')}
      >
        Deny
      </Button>
    </div>
  </div>

  {#if variant === 'bucket'}
    <PolicyPrincipalField
      bind:principal={statement.Principal}
      {readonly}
      onchange={() => emit()}
    />
  {/if}

  <div class="space-y-1.5">
    <Label>Actions</Label>
    <PolicyActionField bind:actions={statement.Action} {readonly} onchange={emit} />
  </div>

  <div class="space-y-1.5">
    <Label>Resources</Label>
    <PolicyResourceField
      bind:resources={statement.Resource}
      {accessibleBuckets}
      {bucketsLoading}
      {readonly}
      onchange={emit}
    />
  </div>
</div>
