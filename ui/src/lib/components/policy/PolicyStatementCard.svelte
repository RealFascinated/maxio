<script lang="ts">
  import ChevronDown from 'lucide-svelte/icons/chevron-down'
  import ChevronUp from 'lucide-svelte/icons/chevron-up'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import { Button } from '$lib/components/ui/button'
  import { Label } from '$lib/components/ui/label'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import * as Tooltip from '$lib/components/ui/tooltip'
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
    statement = $bindable(),
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

{#snippet iconAction(label: string, disabled: boolean, onclick: () => void, icon: typeof Trash2)}
  {@const Icon = icon}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button {...props} type="button" variant="ghost" size="icon-xs" {disabled} aria-label={label} {onclick}>
          <Icon />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<section class="rounded-surface border border-border bg-panel">
  <header class="flex h-9 items-center justify-between gap-2 border-b border-border px-3">
    <div class="flex items-center gap-2">
      <span class="num text-xs text-muted-foreground">Statement {index + 1}</span>
      <SegmentedControl
        size="xs"
        aria-label="Effect"
        value={statement.Effect}
        options={[
          { value: 'Allow', label: 'Allow', disabled: readonly },
          { value: 'Deny', label: 'Deny', disabled: readonly },
        ]}
        onValueChange={(next) => setEffect(next as 'Allow' | 'Deny')}
      />
    </div>
    {#if !readonly && total > 1}
      <div class="flex items-center">
        {@render iconAction('Move statement up', index === 0, () => onmove?.(-1), ChevronUp)}
        {@render iconAction('Move statement down', index >= total - 1, () => onmove?.(1), ChevronDown)}
        {@render iconAction('Delete statement', false, () => ondelete?.(), Trash2)}
      </div>
    {/if}
  </header>

  <div class="divide-y divide-border">
    {#if variant === 'bucket'}
      <div class="grid gap-1.5 px-3 py-2 sm:grid-cols-[6rem_1fr] sm:gap-3">
        <Label class="sm:h-7">Principal</Label>
        <PolicyPrincipalField bind:principal={statement.Principal} {readonly} onchange={() => emit()} />
      </div>
    {/if}

    <div class="grid gap-1.5 px-3 py-2 sm:grid-cols-[6rem_1fr] sm:gap-3">
      <Label class="sm:h-7">Actions</Label>
      <PolicyActionField bind:actions={statement.Action} {readonly} onchange={emit} />
    </div>

    <div class="grid gap-1.5 px-3 py-2 sm:grid-cols-[6rem_1fr] sm:gap-3">
      <Label class="sm:h-7">Resources</Label>
      <PolicyResourceField
        bind:resources={statement.Resource}
        {accessibleBuckets}
        {bucketsLoading}
        {readonly}
        onchange={emit}
      />
    </div>
  </div>
</section>
