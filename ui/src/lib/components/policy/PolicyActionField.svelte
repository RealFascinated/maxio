<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { ACTION_PRESETS, S3_ACTIONS, actionsByCategory } from '$lib/policy/catalog'

  interface Props {
    actions: string[]
    readonly?: boolean
    onchange?: (actions: string[]) => void
  }

  let { actions = $bindable([]), readonly = false, onchange }: Props = $props()

  let customAction = $state('')

  const groups = actionsByCategory()
  const catalogValues = new Set(S3_ACTIONS.map((e) => e.value))

  const customActions = $derived(actions.filter((a) => !catalogValues.has(a)))

  function presetActive(presetActions: string[]): boolean {
    if (actions.length !== presetActions.length) return false
    const set = new Set(actions)
    return presetActions.every((a) => set.has(a))
  }

  function toggle(action: string) {
    if (readonly) return
    if (actions.includes(action)) {
      actions = actions.filter((a) => a !== action)
    } else {
      actions = [...actions, action]
    }
    onchange?.(actions)
  }

  function applyPreset(presetActions: string[]) {
    if (readonly) return
    actions = [...presetActions]
    onchange?.(actions)
  }

  function addCustom() {
    const trimmed = customAction.trim()
    if (!trimmed || readonly) return
    if (!actions.includes(trimmed)) {
      actions = [...actions, trimmed]
      onchange?.(actions)
    }
    customAction = ''
  }

  function removeAction(action: string) {
    if (readonly) return
    actions = actions.filter((a) => a !== action)
    onchange?.(actions)
  }
</script>

<div class="space-y-3">
  {#if !readonly}
    <div class="flex flex-wrap gap-2">
      {#each ACTION_PRESETS as preset (preset.id)}
        <Button
          type="button"
          variant={presetActive(preset.actions) ? 'highlighted' : 'outline'}
          size="sm"
          onclick={() => applyPreset(preset.actions)}
        >
          {preset.label}
        </Button>
      {/each}
    </div>
  {/if}

  <div class="grid gap-3 sm:grid-cols-2">
    {#each groups as group (group.category)}
      <div class="space-y-1.5">
        <span class="text-xs font-medium uppercase tracking-wide text-muted-foreground">{group.label}</span>
        <div class="flex flex-col gap-1">
          {#each group.actions as entry (entry.value)}
            <label class="flex cursor-pointer items-center gap-2 text-sm">
              <input
                type="checkbox"
                class="size-3.5 accent-coollabs"
                checked={actions.includes(entry.value)}
                disabled={readonly}
                onchange={() => toggle(entry.value)}
              />
              <span class="font-mono text-xs">{entry.value}</span>
            </label>
          {/each}
        </div>
      </div>
    {/each}
  </div>

  {#if !readonly || customActions.length}
    <div class="space-y-2">
      {#if !readonly}
        <div class="flex gap-2">
          <Input
            type="text"
            placeholder="Custom action, e.g. s3:GetObjectAcl"
            class="font-mono text-xs"
            bind:value={customAction}
            onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), addCustom())}
          />
          <Button type="button" variant="outline" size="sm" onclick={addCustom}>Add</Button>
        </div>
      {/if}
      {#if customActions.length}
        <div class="flex flex-wrap gap-1.5">
          {#each customActions as action (action)}
            <span
              class="inline-flex items-center gap-1 rounded-sm border-2 border-border bg-muted/40 px-2 py-0.5 font-mono text-xs"
            >
              {action}
              {#if !readonly}
                <button
                  type="button"
                  class="text-muted-foreground hover:text-foreground"
                  aria-label={`Remove ${action}`}
                  onclick={() => removeAction(action)}
                >
                  ×
                </button>
              {/if}
            </span>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
