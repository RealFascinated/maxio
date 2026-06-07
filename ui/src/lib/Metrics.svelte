<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Callout } from '$lib/components/ui/callout'
  import { Badge } from '$lib/components/ui/badge'
  import BarChart2 from 'lucide-svelte/icons/bar-chart-2'
  import { metricsKeys } from '$lib/api/keys'
  import { fetchMetrics } from '$lib/api/metrics'

  const metricsQuery = createQuery(() => ({
    queryKey: metricsKeys.snapshot(),
    queryFn: fetchMetrics,
    refetchInterval: 1_000,
  }))

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B'
    const units = ['B', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(1024))
    const value = bytes / Math.pow(1024, i)
    return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
  }

  function formatUptime(seconds: number): string {
    const total = Math.floor(seconds)
    const days = Math.floor(total / 86_400)
    const hours = Math.floor((total % 86_400) / 3_600)
    const minutes = Math.floor((total % 3_600) / 60)
    const parts: string[] = []
    if (days > 0) parts.push(`${days}d`)
    if (hours > 0 || days > 0) parts.push(`${hours}h`)
    parts.push(`${minutes}m`)
    return parts.join(' ')
  }

  function formatDuration(seconds: number): string {
    if (seconds < 0.001) return '<1 ms'
    if (seconds < 1) return `${(seconds * 1000).toFixed(1)} ms`
    if (seconds < 60) return `${seconds.toFixed(2)} s`
    const mins = Math.floor(seconds / 60)
    const secs = seconds % 60
    return `${mins}m ${secs.toFixed(0)}s`
  }

  function hitRate(hits: number, misses: number): string {
    const total = hits + misses
    if (total === 0) return '—'
    return `${((hits / total) * 100).toFixed(1)}%`
  }
</script>

<div class="mx-auto max-w-5xl space-y-6">
  <div class="flex items-center gap-3">
    <BarChart2 class="size-6 text-coollabs dark:text-warning" />
    <h1 class="text-2xl font-bold dark:text-white">Metrics</h1>
  </div>

  {#if metricsQuery.isPending}
    <p class="text-sm text-neutral-500">Loading metrics…</p>
  {:else if metricsQuery.isError}
    <Callout type="danger">
      Failed to load metrics. {#if metricsQuery.error instanceof Error}{metricsQuery.error.message}{/if}
    </Callout>
  {:else if metricsQuery.data}
    {@const data = metricsQuery.data}

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <h2 class="mb-3 text-lg font-bold dark:text-white">Server</h2>
      <dl class="grid gap-3 sm:grid-cols-2">
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Uptime</dt>
          <dd class="text-lg font-semibold dark:text-white">{formatUptime(data.uptimeSeconds)}</dd>
        </div>
      </dl>
    </section>

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <h2 class="text-lg font-bold dark:text-white">Cache</h2>
        {#if data.cache.enabled}
          <Badge variant="secondary">Enabled</Badge>
        {:else}
          <Badge variant="outline">Disabled</Badge>
        {/if}
        {#if data.cache.writebackHalted}
          <Badge variant="destructive">Writeback halted</Badge>
        {/if}
      </div>
      <dl class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Hits</dt>
          <dd class="text-lg font-semibold dark:text-white">{data.cache.hits.toLocaleString()}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Misses</dt>
          <dd class="text-lg font-semibold dark:text-white">{data.cache.misses.toLocaleString()}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Hit rate</dt>
          <dd class="text-lg font-semibold dark:text-white">
            {hitRate(data.cache.hits, data.cache.misses)}
          </dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Evictions</dt>
          <dd class="text-lg font-semibold dark:text-white">{data.cache.evictions.toLocaleString()}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Size</dt>
          <dd class="text-lg font-semibold dark:text-white">
            {formatBytes(data.cache.sizeBytes)}
            {#if data.cache.maxSizeBytes > 0}
              <span class="text-sm font-normal text-neutral-500">
                / {formatBytes(data.cache.maxSizeBytes)}
              </span>
            {/if}
          </dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Entries</dt>
          <dd class="text-lg font-semibold dark:text-white">{data.cache.entries.toLocaleString()}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Dirty objects</dt>
          <dd class="text-lg font-semibold dark:text-white">{data.cache.dirtyObjects.toLocaleString()}</dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Populated</dt>
          <dd class="text-lg font-semibold dark:text-white">{formatBytes(data.cache.populateBytes)}</dd>
        </div>
      </dl>
    </section>

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <h2 class="mb-3 text-lg font-bold dark:text-white">Storage operations</h2>
      {#if data.storageOps.length === 0}
        <p class="text-sm text-neutral-500">No storage operations recorded yet.</p>
      {:else}
        <Table.Root>
          <Table.Header>
            <Table.Row>
              <Table.Head>Operation</Table.Head>
              <Table.Head class="text-right">Count</Table.Head>
              <Table.Head class="text-right">Avg latency</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {#each data.storageOps as op}
              <Table.Row>
                <Table.Cell class="font-mono text-sm">{op.operation}</Table.Cell>
                <Table.Cell class="text-right">{op.count.toLocaleString()}</Table.Cell>
                <Table.Cell class="text-right">
                  {formatDuration(op.count > 0 ? op.sumSeconds / op.count : 0)}
                </Table.Cell>
              </Table.Row>
            {/each}
          </Table.Body>
        </Table.Root>
      {/if}
    </section>

    {#if data.process}
      <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
        <h2 class="mb-3 text-lg font-bold dark:text-white">Process</h2>
        <dl class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Resident memory</dt>
            <dd class="text-lg font-semibold dark:text-white">
              {formatBytes(data.process.residentMemoryBytes)}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Virtual memory</dt>
            <dd class="text-lg font-semibold dark:text-white">
              {formatBytes(data.process.virtualMemoryBytes)}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">CPU usage</dt>
            <dd class="text-lg font-semibold dark:text-white">
              {data.process.cpuUsagePercent.toFixed(1)}%
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Open file descriptors</dt>
            <dd class="text-lg font-semibold dark:text-white">
              {data.process.openFds.toLocaleString()}
              {#if data.process.maxFds > 0}
                <span class="text-sm font-normal text-neutral-500">
                  / {data.process.maxFds.toLocaleString()}
                </span>
              {/if}
            </dd>
          </div>
        </dl>
      </section>
    {/if}
  {/if}
</div>
