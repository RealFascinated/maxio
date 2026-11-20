<script lang="ts">
  import X from 'lucide-svelte/icons/x'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Tag } from '$lib/components/ui/tag'
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

<div class="flex min-w-0 flex-col gap-2">
  {#if !readonly}
    <div class="flex flex-wrap gap-1.5">
      {#each ACTION_PRESETS as preset (preset.id)}
        <Button
          type="button"
          variant={presetActive(preset.actions) ? 'default' : 'outline'}
          size="xs"
          onclick={() => applyPreset(preset.actions)}
        >
          {preset.label}
        </Button>
      {/each}
    </div>
  {/if}

  <div class="grid max-h-56 gap-x-4 gap-y-2 overflow-y-auto sm:grid-cols-2">
    {#each groups as group (group.category)}
      <div class="flex flex-col">
        <span class="h-6 text-xs font-medium text-muted-foreground">{group.label}</span>
        {#each group.actions as entry (entry.value)}
          <label class="flex h-6 cursor-pointer items-center gap-2 rounded-control px-1 hover:bg-hover">
            <input
              type="checkbox"
              class="size-3.5 accent-accent"
              checked={actions.includes(entry.value)}
              disabled={readonly}
              onchange={() => toggle(entry.value)}
            />
            <span class="truncate font-mono text-xs">{entry.value}</span>
          </label>
        {/each}
      </div>
    {/each}
  </div>

  {#if !readonly}
    <div class="flex gap-2">
      <Input
        type="text"
        placeholder="Custom action, e.g. s3:GetObjectAcl"
        class="font-mono text-xs"
        aria-label="Custom action"
        bind:value={customAction}
        onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), addCustom())}
      />
      <Button type="button" variant="secondary" onclick={addCustom}>Add</Button>
    </div>
  {/if}
  {#if customActions.length}
    <div class="flex flex-wrap gap-1.5">
      {#each customActions as action (action)}
        <Tag class="gap-1 font-mono">
          {action}
          {#if !readonly}
            <button
              type="button"
              class="inline-flex size-4 shrink-0 items-center justify-center rounded-control hover:text-foreground"
              aria-label={`Remove action ${action}`}
              onclick={() => removeAction(action)}
            >
              <X class="size-3" />
            </button>
          {/if}
        </Tag>
      {/each}
    </div>
  {/if}
</div>
