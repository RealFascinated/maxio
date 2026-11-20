<script lang="ts">
  import { formatBytes } from '$lib/format-bytes'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import BarList from './BarList.svelte'
  import ChartPanel from './ChartPanel.svelte'
  import Panel from './Panel.svelte'
  import {
    COLORS,
    formatCompact,
    formatPercent,
    formatRate,
    formatSeconds,
    percentOf,
    seriesOf,
  } from './chart-utils'
  import type { MetricsView } from './metrics-view'

  let { view }: { view: MetricsView } = $props()

  const s = (key: Parameters<typeof seriesOf>[1]) => seriesOf(view.history, key)

  const latencyKinds = [
    { value: 'http', label: 'HTTP' },
    { value: 'read', label: 'Read' },
    { value: 'write', label: 'Write' },
  ]
  let latencyKind = $state('http')
  const latencyKey = (percentile: 'P50' | 'P95' | 'P99') =>
    `${latencyKind}Latency${percentile}` as Parameters<typeof seriesOf>[1]

  const http = $derived(view.snapshot.http)
  const statusItems = $derived([
    { label: '2xx', value: http.status2xx, color: COLORS.s2xx },
    { label: '3xx', value: http.status3xx, color: COLORS.s3xx },
    { label: '4xx', value: http.status4xx, color: COLORS.s4xx },
    { label: '5xx', value: http.status5xx, color: COLORS.s5xx },
  ])
  const share = (value: number) =>
    http.totalRequests > 0 ? `${((value / http.totalRequests) * 100).toFixed(2)}%` : '0%'
</script>

<div class="grid gap-2 lg:grid-cols-2">
  <ChartPanel
    {view}
    title="Bytes in/out"
    mode="area"
    bytes
    format={(value) => `${formatBytes(value)}/s`}
    axisFormat={formatBytes}
    series={[
      { key: 'read', label: 'Out (read)', color: COLORS.read, values: s('readBps') },
      { key: 'write', label: 'In (write)', color: COLORS.write, values: s('writeBps') },
    ]}
  />
  <ChartPanel
    {view}
    title="Requests/s by status"
    mode="stacked"
    format={formatCompact}
    series={[
      { key: '2xx', label: '2xx', color: COLORS.s2xx, values: s('req2xx') },
      { key: '3xx', label: '3xx', color: COLORS.s3xx, values: s('req3xx') },
      { key: '4xx', label: '4xx', color: COLORS.s4xx, values: s('req4xx') },
      { key: '5xx', label: '5xx', color: COLORS.s5xx, values: s('req5xx') },
    ]}
  />
  <ChartPanel
    {view}
    title="Latency"
    format={formatSeconds}
    series={[
      { key: 'p50', label: 'p50', color: COLORS.p50, values: s(latencyKey('P50')) },
      { key: 'p95', label: 'p95', color: COLORS.p95, values: s(latencyKey('P95')) },
      { key: 'p99', label: 'p99', color: COLORS.p99, values: s(latencyKey('P99')) },
    ]}
  >
    {#snippet actions()}
      <SegmentedControl
        size="xs"
        aria-label="Latency kind"
        options={latencyKinds}
        bind:value={latencyKind}
      />
    {/snippet}
  </ChartPanel>
  <ChartPanel
    {view}
    title="Error rate"
    format={formatPercent}
    series={[
      { key: '4xx', label: '4xx', color: COLORS.s4xx, values: percentOf(s('req4xx'), s('reqPerSec')) },
      { key: '5xx', label: '5xx', color: COLORS.s5xx, values: percentOf(s('req5xx'), s('reqPerSec')) },
    ]}
  />
  <ChartPanel
    {view}
    title="IOPS"
    mode="stacked"
    format={formatRate}
    series={[
      { key: 'read', label: 'Read', color: COLORS.read, values: s('readIops') },
      { key: 'write', label: 'Write', color: COLORS.write, values: s('writeIops') },
      { key: 'meta', label: 'Metadata', color: COLORS.meta, values: s('metaIops') },
    ]}
  />
  <ChartPanel
    {view}
    title="Active clients"
    mode="area"
    format={(value) => Math.round(value).toLocaleString()}
    series={[{ key: 'clients', label: 'Clients', color: COLORS.palette[2], values: s('activeClients') }]}
  />
  <Panel title="Responses since start" class="lg:col-span-2">
    <div class="grid gap-4 md:grid-cols-[2fr_1fr]">
      <BarList
        items={statusItems.map((item) => ({
          label: item.label,
          value: item.value,
          color: item.color,
          detail: share(item.value),
        }))}
        format={(value) => Math.round(value).toLocaleString()}
      />
      <dl class="grid grid-cols-[auto_1fr] content-start gap-x-4 gap-y-1">
        <dt class="text-muted-foreground">Requests</dt>
        <dd class="num text-right">{http.totalRequests.toLocaleString()}</dd>
        <dt class="text-muted-foreground">Bytes read</dt>
        <dd class="num text-right">{formatBytes(http.bytesRead)}</dd>
        <dt class="text-muted-foreground">Bytes written</dt>
        <dd class="num text-right">{formatBytes(http.bytesWritten)}</dd>
      </dl>
    </div>
  </Panel>
</div>
