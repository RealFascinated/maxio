<script lang="ts">
  import type { ComponentProps, Snippet } from 'svelte'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import Panel from './Panel.svelte'
  import TimeSeriesChart from './TimeSeriesChart.svelte'
  import type { MetricsView } from './metrics-view'

  type Props = Omit<ComponentProps<typeof TimeSeriesChart>, 't' | 'range' | 'label'> & {
    view: MetricsView
    title: string
    actions?: Snippet
  }

  let { view, title, actions, series, height = 180, class: className, ...chart }: Props = $props()

  const collecting = $derived(
    !series.some((item) => item.values.filter((value) => value !== null).length >= 3),
  )
</script>

<Panel {title} {actions} class={className}>
  {#if view.historyStatus === 'loading'}
    <Skeleton class="rounded-control" style="height: {height}px" />
  {:else if view.historyStatus === 'forbidden'}
    <p class="flex items-center justify-center text-sm text-muted-foreground" style="height: {height}px">
      Root account required
    </p>
  {:else if view.historyStatus === 'error' || !view.history}
    <p class="flex items-center justify-center text-sm text-muted-foreground" style="height: {height}px">
      History unavailable
    </p>
  {:else if collecting}
    <p class="flex items-center justify-center text-sm text-muted-foreground" style="height: {height}px">
      Collecting data
    </p>
  {:else}
    <TimeSeriesChart {...chart} {series} {height} t={view.history.t} range={view.range} label={title} />
  {/if}
</Panel>
