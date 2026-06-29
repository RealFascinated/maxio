<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Label } from '$lib/components/ui/label'
  import { prettyPolicyJson } from '$lib/policy/document'
  import type { PolicyValidationIssue } from '$lib/policy/validation'

  interface Props {
    value: string
    readonly?: boolean
    issues?: PolicyValidationIssue[]
    onchange?: (value: string) => void
  }

  let { value = $bindable(''), readonly = false, issues = [], onchange }: Props = $props()

  let localError = $state<string | null>(null)

  function formatJson() {
    const pretty = prettyPolicyJson(value)
    if (!pretty) {
      localError = 'Policy document must be valid JSON'
      return
    }
    localError = null
    value = pretty
    onchange?.(value)
  }

  const errors = $derived(issues.filter((i) => !i.warning))
  const warnings = $derived(issues.filter((i) => i.warning))
</script>

<div class="space-y-2">
  {#if !readonly}
    <div class="flex justify-end">
      <Button type="button" variant="outline" size="sm" onclick={formatJson}>Format</Button>
    </div>
  {/if}
  {#if localError}
    <Callout type="danger">{localError}</Callout>
  {/if}
  {#each errors as issue (issue.message)}
    <Callout type="danger">{issue.message}</Callout>
  {/each}
  {#each warnings as issue (issue.message)}
    <Callout type="warning">{issue.message}</Callout>
  {/each}
  <div class="space-y-1.5">
    <Label for="policyJsonPanel">Policy document (JSON)</Label>
    <textarea
      id="policyJsonPanel"
      bind:value
      readonly={readonly}
      oninput={() => onchange?.(value)}
      class="input-cool h-80 w-full resize-none overflow-auto rounded-sm bg-background p-3 font-mono text-xs"
    ></textarea>
  </div>
</div>
