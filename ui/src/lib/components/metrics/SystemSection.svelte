<script lang="ts">
  import { formatBytes } from '$lib/format-bytes'
  import { formatUptime } from '$lib/format'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import ChartPanel from './ChartPanel.svelte'
  import Panel from './Panel.svelte'
  import { COLORS, formatPercent, maxValue, seriesOf } from './chart-utils'
  import type { MetricsView } from './metrics-view'

  let { view }: { view: MetricsView } = $props()

  const s = (key: Parameters<typeof seriesOf>[1]) => seriesOf(view.history, key)
  const process = $derived(view.snapshot.process)
  const fdPercent = $derived(
    process && process.maxFds > 0 ? (process.openFds / process.maxFds) * 100 : null,
  )

  const facts = $derived([
    { label: 'Version', value: view.snapshot.version, percent: null },
    { label: 'Started', value: new Date(view.snapshot.startedAt).toLocaleString(), percent: null },
    { label: 'Uptime', value: formatUptime(view.snapshot.uptimeSeconds), percent: null },
    ...(process
      ? [
          { label: 'CPU', value: formatPercent(process.cpuUsagePercent), percent: null },
          { label: 'Resident memory', value: formatBytes(process.residentMemoryBytes), percent: null },
          { label: 'Virtual memory', value: formatBytes(process.virtualMemoryBytes), percent: null },
          {
            label: 'Open file descriptors',
            value: `${process.openFds.toLocaleString()}${process.maxFds > 0 ? ` / ${process.maxFds.toLocaleString()}` : ''}`,
            percent: fdPercent,
          },
        ]
      : []),
  ])
</script>

<div class="flex flex-col gap-3">
  <Panel title="Process">
    <dl class="grid gap-x-8 sm:grid-cols-2">
      {#each facts as fact (fact.label)}
        <div class="flex min-h-8 min-w-0 flex-col justify-center gap-1 border-b border-border py-1 last:border-0 sm:[&:nth-last-child(2):nth-child(odd)]:border-0">
          <div class="flex items-baseline justify-between gap-3">
            <dt class="shrink-0 text-muted-foreground">{fact.label}</dt>
            <dd class="num truncate">{fact.value}</dd>
          </div>
          {#if fact.percent != null}
            <ProgressBar class="mt-1" value={fact.percent} warnAt={70} dangerAt={90} aria-label="{fact.label} used" />
          {/if}
        </div>
      {/each}
    </dl>
  </Panel>

  <div class="grid gap-2 lg:grid-cols-2">
    <ChartPanel
      {view}
      title="CPU"
      mode="area"
      format={formatPercent}
      series={[{ key: 'cpu', label: 'CPU', color: COLORS.palette[1], values: s('cpuPercent') }]}
    />
    <ChartPanel
      {view}
      title="Resident memory"
      mode="area"
      bytes
      format={formatBytes}
      series={[{ key: 'rss', label: 'RSS', color: COLORS.read, values: s('rssBytes') }]}
    />
    <ChartPanel
      {view}
      title="Open file descriptors"
      class="lg:col-span-2"
      mode="area"
      format={(value) => Math.round(value).toLocaleString()}
      refLine={process && process.maxFds > 0 && process.maxFds <= 5 * maxValue(s('openFds')) ? { value: process.maxFds, label: `limit ${process.maxFds.toLocaleString()}` } : undefined}
      series={[{ key: 'fds', label: 'Open', color: COLORS.palette[2], values: s('openFds') }]}
    />
  </div>
</div>
