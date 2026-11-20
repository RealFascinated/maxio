<script lang="ts">
  import type { CacheSnapshot } from '$lib/api/metrics'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import * as Table from '$lib/components/ui/table'
  import { formatMetricName } from '$lib/format'
  import { formatBytes } from '$lib/format-bytes'
  import ChartPanel from './ChartPanel.svelte'
  import Panel from './Panel.svelte'
  import { COLORS, formatPercent } from './chart-utils'
  import type { MetricsView } from './metrics-view'

  let { view }: { view: MetricsView } = $props()

  const diskCache = $derived(view.snapshot.caches.find((cache) => cache.id === 'object_disk'))
  const otherCaches = $derived(view.snapshot.caches.filter((cache) => cache.id !== 'object_disk'))
  const isIdle = (cache: CacheSnapshot) => cache.hits === 0 && cache.misses === 0 && cache.entries === 0
  const idleCount = $derived(otherCaches.filter(isIdle).length)
  let showIdle = $state(false)
  const rows = $derived(showIdle ? otherCaches : otherCaches.filter((cache) => !isIdle(cache)))

  const hitRatio = (cache: CacheSnapshot) =>
    cache.hits + cache.misses > 0 ? cache.hits / (cache.hits + cache.misses) : null

  const hitRateSeries = $derived(
    Object.entries(view.history?.cacheHitRates ?? {})
      .filter(([, values]) => values.some((value) => value !== null))
      .map(([id, values], index) => ({
        key: id,
        label: formatMetricName(id).replace(/ Cache$/, ''),
        color: COLORS.palette[index % COLORS.palette.length],
        values: values.map((value) => (value === null ? null : value * 100)),
      })),
  )

  const diskFacts = $derived(
    diskCache
      ? [
          { label: 'Hits', value: diskCache.hits.toLocaleString() },
          { label: 'Misses', value: diskCache.misses.toLocaleString() },
          { label: 'Hit rate', value: hitRatio(diskCache) === null ? '—' : formatPercent(hitRatio(diskCache)! * 100) },
          { label: 'Entries', value: diskCache.entries.toLocaleString() },
          { label: 'Evictions', value: diskCache.evictions.toLocaleString() },
          { label: 'Dirty objects', value: diskCache.dirtyObjects.toLocaleString() },
          { label: 'Dirty bytes', value: formatBytes(diskCache.dirtyBytes) },
        ]
      : [],
  )
</script>

<div class="flex flex-col gap-3">
  {#if diskCache}
    <Panel title="Object disk cache">
      {#snippet actions()}
        <span class="flex items-center gap-3 text-xs">
          {#if diskCache.writebackHalted}
            <Badge variant="error" label="Writeback halted" />
          {/if}
          <Badge variant={diskCache.enabled ? 'success' : 'neutral'} label={diskCache.enabled ? 'Enabled' : 'Disabled'} />
        </span>
      {/snippet}
      {#if !diskCache.enabled}
        <p class="text-muted-foreground">Disabled</p>
      {:else}
        {#if diskCache.maxSizeBytes > 0}
          {@const percent = Math.min(100, (diskCache.sizeBytes / diskCache.maxSizeBytes) * 100)}
          <div class="mb-3">
            <div class="mb-1 flex items-baseline justify-between">
              <span class="text-muted-foreground">Capacity</span>
              <span class="num">
                {formatBytes(diskCache.sizeBytes)} / {formatBytes(diskCache.maxSizeBytes)}
                <span class="ml-1 text-xs text-muted-foreground">{percent.toFixed(1)}%</span>
              </span>
            </div>
            <ProgressBar value={percent} warnAt={80} dangerAt={95} aria-label="Cache capacity used" />
          </div>
        {/if}
        <dl class="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4 lg:grid-cols-7">
          {#each diskFacts as fact (fact.label)}
            <div>
              <dt class="text-xs text-muted-foreground">{fact.label}</dt>
              <dd class="num font-medium">{fact.value}</dd>
            </div>
          {/each}
        </dl>
      {/if}
    </Panel>
  {/if}

  <ChartPanel {view} title="Hit rate" format={formatPercent} yMax={100} height={220} series={hitRateSeries} />

  <section class="flex flex-col gap-2">
    <h2 class="text-sm">Caches</h2>
    <Table.Root class="min-w-[34rem]">
      <Table.Header>
        <Table.Row class="hover:bg-transparent">
          <Table.Head>Cache</Table.Head>
          <Table.Head class="text-right">Hits</Table.Head>
          <Table.Head class="text-right">Misses</Table.Head>
          <Table.Head class="text-right">Hit rate</Table.Head>
          <Table.Head class="text-right">Evictions</Table.Head>
          <Table.Head class="text-right">Entries</Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each rows as cache (cache.id)}
          {@const ratio = hitRatio(cache)}
          <Table.Row>
            <Table.Cell class="font-medium">{formatMetricName(cache.id)}</Table.Cell>
            <Table.Cell class="num text-right">{cache.hits.toLocaleString()}</Table.Cell>
            <Table.Cell class="num text-right">{cache.misses.toLocaleString()}</Table.Cell>
            <Table.Cell>
              <div class="flex items-center justify-end gap-2">
                <ProgressBar class="w-16" value={(ratio ?? 0) * 100} />
                <span class="num w-12 text-right">{ratio === null ? '—' : formatPercent(ratio * 100)}</span>
              </div>
            </Table.Cell>
            <Table.Cell class="num text-right">{cache.evictions.toLocaleString()}</Table.Cell>
            <Table.Cell class="num text-right">{cache.entries.toLocaleString()}</Table.Cell>
          </Table.Row>
        {:else}
          <Table.Row class="hover:bg-transparent">
            <Table.Cell colspan={6} class="text-center text-muted-foreground">No active caches</Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
    {#if idleCount > 0}
      <Button variant="ghost" size="xs" class="self-start" aria-expanded={showIdle} onclick={() => (showIdle = !showIdle)}>
        {showIdle ? 'Hide idle' : `Show ${idleCount} idle`}
      </Button>
    {/if}
  </section>
</div>
