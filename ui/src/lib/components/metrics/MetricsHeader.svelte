<script lang="ts">
  import { METRICS_RANGES, type MetricsRange } from '$lib/api/metrics'
  import { Button } from '$lib/components/ui/button'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import { Select } from '$lib/components/ui/select'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import RefreshCw from 'lucide-svelte/icons/refresh-cw'
  import { cn } from '$lib/utils'
  import { REFRESH_OPTIONS } from './metrics-view'

  interface Props {
    range: MetricsRange
    onrange: (range: MetricsRange) => void
    refreshSeconds: string
    onrefresh: (seconds: string) => void
    lastUpdated: number
    fetching: boolean
    onreload: () => void
  }

  let { range, onrange, refreshSeconds, onrefresh, lastUpdated, fetching, onreload }: Props = $props()

  const rangeOptions = METRICS_RANGES.map((value) => ({ value, label: value }))
  const updatedAt = $derived(lastUpdated > 0 ? new Date(lastUpdated).toLocaleTimeString() : null)
</script>

<PageHeader title="Metrics">
  {#snippet actions()}
    {#if updatedAt}
      <span class="num hidden text-xs text-muted-foreground sm:inline">{updatedAt}</span>
    {/if}
    <SegmentedControl
      aria-label="Time range"
      options={rangeOptions}
      value={range}
      onValueChange={(value) => onrange(value as MetricsRange)}
    />
    <Select
      aria-label="Auto-refresh interval"
      class="w-20"
      value={refreshSeconds}
      options={REFRESH_OPTIONS.map((option) => ({ value: option.value, label: option.label }))}
      onValueChange={onrefresh}
    />
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" size="icon" aria-label="Refresh now" onclick={onreload}>
            <RefreshCw class={cn(fetching && 'animate-spin')} />
          </Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Refresh now</Tooltip.Content>
    </Tooltip.Root>
  {/snippet}
</PageHeader>
