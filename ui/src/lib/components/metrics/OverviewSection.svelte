<script lang="ts">
  import { formatBytes } from '$lib/format-bytes'
  import ChartPanel from './ChartPanel.svelte'
  import HealthBanner from './HealthBanner.svelte'
  import KpiCard from './KpiCard.svelte'
  import {
    COLORS,
    deltaPercent,
    formatBytesPerSec,
    formatCompact,
    formatSeconds,
    lastValue,
    percentOf,
    seriesOf,
  } from './chart-utils'
  import { deriveHealth, type MetricsView, type SectionId } from './metrics-view'

  interface Props {
    view: MetricsView
    onselect: (section: SectionId) => void
  }

  let { view, onselect }: Props = $props()

  const snapshot = $derived(view.snapshot)
  const s = (key: Parameters<typeof seriesOf>[1]) => seriesOf(view.history, key)
  const errorRate = $derived(percentOf(s('req5xx'), s('reqPerSec')))
  const requestRate = $derived(lastValue(s('reqPerSec')))
  const lastError = $derived(lastValue(errorRate))
  const p95 = $derived(snapshot.latencyPercentiles.http?.p95 ?? null)
</script>

<div class="flex flex-col gap-3">
  <HealthBanner issues={deriveHealth(view)} {onselect} />

  <div class="grid grid-cols-2 gap-2 lg:grid-cols-4">
    <KpiCard
      label="Read throughput"
      value={formatBytesPerSec(snapshot.throughput.readBytesPerSec)}
      values={s('readBps')}
      delta={deltaPercent(s('readBps'))}
      color={COLORS.read}
    />
    <KpiCard
      label="Write throughput"
      value={formatBytesPerSec(snapshot.throughput.writeBytesPerSec)}
      values={s('writeBps')}
      delta={deltaPercent(s('writeBps'))}
      color={COLORS.write}
    />
    <KpiCard
      label="Requests/s"
      value={requestRate === null ? '—' : formatCompact(requestRate)}
      values={s('reqPerSec')}
      delta={deltaPercent(s('reqPerSec'))}
    />
    <KpiCard
      label="5xx rate"
      value={lastError === null ? '—' : `${lastError.toFixed(2)}%`}
      values={errorRate}
      delta={deltaPercent(errorRate)}
      color={COLORS.s5xx}
    />
    <KpiCard
      label="p95 latency"
      value={p95 === null ? '—' : formatSeconds(p95)}
      values={s('httpLatencyP95')}
      delta={deltaPercent(s('httpLatencyP95'))}
      color={COLORS.p95}
    />
    <KpiCard
      label="Active clients"
      value={snapshot.activeClients.toLocaleString()}
      values={s('activeClients')}
      delta={deltaPercent(s('activeClients'))}
    />
    <KpiCard
      label="Stored"
      value={formatBytes(snapshot.storageTotals.sizeBytes)}
      values={s('storageBytes')}
      delta={deltaPercent(s('storageBytes'))}
    />
    <KpiCard
      label="Objects"
      value={snapshot.storageTotals.objectCount.toLocaleString()}
      values={s('objectCount')}
      delta={deltaPercent(s('objectCount'))}
    />
  </div>

  <div class="grid gap-2 lg:grid-cols-2">
    <ChartPanel
      {view}
      title="Throughput"
      mode="area"
      bytes
      format={formatBytesPerSec}
      axisFormat={formatBytes}
      series={[
        { key: 'read', label: 'Read', color: COLORS.read, values: s('readBps') },
        { key: 'write', label: 'Write', color: COLORS.write, values: s('writeBps') },
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
      title="HTTP latency"
      format={formatSeconds}
      series={[
        { key: 'p50', label: 'p50', color: COLORS.p50, values: s('httpLatencyP50') },
        { key: 'p95', label: 'p95', color: COLORS.p95, values: s('httpLatencyP95') },
        { key: 'p99', label: 'p99', color: COLORS.p99, values: s('httpLatencyP99') },
      ]}
    />
    <ChartPanel
      {view}
      title="Active clients"
      mode="area"
      format={(value) => Math.round(value).toLocaleString()}
      series={[{ key: 'clients', label: 'Clients', color: COLORS.palette[1], values: s('activeClients') }]}
    />
  </div>
</div>
