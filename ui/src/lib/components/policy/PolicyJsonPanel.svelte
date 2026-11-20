<script lang="ts">
  import { Callout } from '$lib/components/ui/callout'
  import type { PolicyValidationIssue } from '$lib/policy/validation'

  interface Props {
    value: string
    readonly?: boolean
    issues?: PolicyValidationIssue[]
    onchange?: (value: string) => void
  }

  let { value = $bindable(''), readonly = false, issues = [], onchange }: Props = $props()

  const errors = $derived(issues.filter((i) => !i.warning))
  const warnings = $derived(issues.filter((i) => i.warning))
</script>

<div class="flex flex-col gap-2">
  {#each errors as issue (issue.message)}
    <Callout type="danger">{issue.message}</Callout>
  {/each}
  {#each warnings as issue (issue.message)}
    <Callout type="warning">{issue.message}</Callout>
  {/each}
  <textarea
    aria-label="Policy document JSON"
    bind:value
    readonly={readonly}
    spellcheck="false"
    oninput={() => onchange?.(value)}
    class="h-80 w-full resize-none overflow-auto rounded-control border border-border-strong bg-inset p-2 font-mono text-xs text-foreground outline-none transition-colors hover:border-subtle focus:border-accent focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-0"
  ></textarea>
</div>
