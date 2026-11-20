<script lang="ts">
  import { base } from '$lib/base'
  import { formatBytes } from '$lib/format-bytes'
  import { StatTile } from '$lib/components/ui/stat-tile'
  import BarList from './BarList.svelte'
  import ChartPanel from './ChartPanel.svelte'
  import Panel from './Panel.svelte'
  import { COLORS, seriesOf } from './chart-utils'
  import type { MetricsView } from './metrics-view'

  let { view }: { view: MetricsView } = $props()

  const s = (key: Parameters<typeof seriesOf>[1]) => seriesOf(view.history, key)
  const totals = $derived(view.snapshot.storageTotals)
  const averageSize = $derived(totals.objectCount > 0 ? totals.sizeBytes / totals.objectCount : 0)
  const averageSeries = $derived(
    s('storageBytes').map((bytes, i) => {
      const count = s('objectCount')[i]
      return bytes === null || !count ? null : bytes / count
    }),
  )

  const bucketItems = (key: 'sizeBytes' | 'objectCount') =>
    view.snapshot.buckets
      .toSorted((a, b) => b[key] - a[key])
      .slice(0, 10)
      .map((bucket) => ({
        label: bucket.name,
        value: bucket[key],
        href: `${base}/buckets/${encodeURIComponent(bucket.name)}`,
      }))

  const facts = $derived([
    { label: 'Buckets', value: totals.bucketCount.toLocaleString() },
    { label: 'Objects', value: totals.objectCount.toLocaleString() },
    { label: 'Total size', value: formatBytes(totals.sizeBytes) },
    { label: 'Avg object size', value: formatBytes(averageSize) },
  ])
</script>

<div class="flex flex-col gap-3">
  <div class="grid grid-cols-2 gap-2 lg:grid-cols-4">
    {#each facts as fact (fact.label)}
      <StatTile label={fact.label} value={fact.value} />
    {/each}
  </div>

  <div class="grid gap-2 lg:grid-cols-2">
    <ChartPanel
      {view}
      title="Total size"
      mode="area"
      bytes
      format={formatBytes}
      series={[{ key: 'size', label: 'Size', color: COLORS.read, values: s('storageBytes') }]}
    />
    <ChartPanel
      {view}
      title="Objects"
      mode="area"
      format={(value) => Math.round(value).toLocaleString()}
      series={[{ key: 'objects', label: 'Objects', color: COLORS.palette[2], values: s('objectCount') }]}
    />
    <ChartPanel
      {view}
      title="Average object size"
      class="lg:col-span-2"
      bytes
      format={formatBytes}
      series={[{ key: 'avg', label: 'Average', color: COLORS.palette[4], values: averageSeries }]}
    />
    <Panel title="Top buckets by size">
      <BarList items={bucketItems('sizeBytes')} format={formatBytes} empty="No buckets" />
    </Panel>
    <Panel title="Top buckets by objects">
      <BarList
        items={bucketItems('objectCount')}
        format={(value) => value.toLocaleString()}
        color={COLORS.palette[2]}
        empty="No buckets"
      />
    </Panel>
  </div>
</div>
