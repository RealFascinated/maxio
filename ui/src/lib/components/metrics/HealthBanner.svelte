<script lang="ts">
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import type { HealthIssue, SectionId } from './metrics-view'

  interface Props {
    issues: HealthIssue[]
    onselect: (section: SectionId) => void
  }

  let { issues, onselect }: Props = $props()
</script>

<div role="status" class="rounded-surface border border-border bg-panel px-3 py-1.5 text-sm">
  {#if issues.length === 0}
    <Badge variant="success" label="Healthy" />
  {:else}
    <ul class="flex flex-col gap-1">
      {#each issues as issue (issue.text)}
        <li class="flex min-h-6 items-center gap-2">
          <Badge variant={issue.severity === 'error' ? 'error' : 'warning'} />
          <span>{issue.text}</span>
          <Button variant="link" size="xs" class="ml-auto" onclick={() => onselect(issue.section)}>View</Button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
