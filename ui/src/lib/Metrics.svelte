<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Callout } from '$lib/components/ui/callout'
  import { Badge } from '$lib/components/ui/badge'
  import BarChart2 from 'lucide-svelte/icons/bar-chart-2'
  import Cpu from 'lucide-svelte/icons/cpu'
  import Database from 'lucide-svelte/icons/database'
  import Download from 'lucide-svelte/icons/download'
  import HardDrive from 'lucide-svelte/icons/hard-drive'
  import Package from 'lucide-svelte/icons/package'
  import Server from 'lucide-svelte/icons/server'
  import Table2 from 'lucide-svelte/icons/table-2'
  import Upload from 'lucide-svelte/icons/upload'
  import { metricsKeys } from '$lib/api/keys'
  import { fetchMetrics, type CacheSnapshot } from '$lib/api/metrics'
  import { formatBytes } from '$lib/format-bytes'
  import { formatDuration, formatMetricName, formatUptime, hitRate } from '$lib/format'

  const metricsQuery = createQuery(() => ({
    queryKey: metricsKeys.snapshot(),
    queryFn: fetchMetrics,
    refetchInterval: 1_000,
  }))

  function formatLatency(seconds: number | null | undefined): string {
    if (seconds == null) return '—'
    return formatDuration(seconds)
  }

  function isObjectDiskCache(cache: CacheSnapshot): boolean {
    return cache.id === 'object_disk'
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
      <div class="mb-3 flex items-center gap-2">
        <Database class="size-5 text-coollabs dark:text-warning" />
        <h2 class="text-lg font-bold dark:text-white">Storage</h2>
      </div>
      <dl class="grid gap-4 sm:grid-cols-3">
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Buckets</dt>
          <dd class="text-lg font-semibold dark:text-white">
            {data.storageTotals.bucketCount.toLocaleString()}
          </dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Objects</dt>
          <dd class="text-lg font-semibold dark:text-white">
            {data.storageTotals.objectCount.toLocaleString()}
          </dd>
        </div>
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Total size</dt>
          <dd class="text-lg font-semibold dark:text-white">
            {formatBytes(data.storageTotals.sizeBytes)}
          </dd>
        </div>
      </dl>
    </section>

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <Server class="size-5 text-coollabs dark:text-warning" />
        <h2 class="text-lg font-bold dark:text-white">Server</h2>
      </div>
      <dl class="grid gap-4 sm:grid-cols-3">
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Uptime</dt>
          <dd class="text-lg font-semibold dark:text-white">{formatUptime(data.uptimeSeconds)}</dd>
        </div>
        <div>
          <dt class="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wide text-neutral-500">
            <Download class="size-3.5" />
            Avg read latency
          </dt>
          <dd class="text-lg font-semibold dark:text-white">{formatLatency(data.latency.readSeconds)}</dd>
          <dd class="text-xs text-neutral-500">Last {data.latency.windowSeconds}s, end-to-end</dd>
        </div>
        <div>
          <dt class="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wide text-neutral-500">
            <Upload class="size-3.5" />
            Avg write latency
          </dt>
          <dd class="text-lg font-semibold dark:text-white">{formatLatency(data.latency.writeSeconds)}</dd>
          <dd class="text-xs text-neutral-500">Last {data.latency.windowSeconds}s, end-to-end</dd>
        </div>
      </dl>
    </section>

    {#each data.caches as cache (cache.id)}
      <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
        <div class="mb-3 flex flex-wrap items-center gap-2">
          <HardDrive class="size-5 shrink-0 text-coollabs dark:text-warning" />
          <h2 class="text-lg font-bold dark:text-white">{formatMetricName(cache.id)}</h2>
          {#if isObjectDiskCache(cache)}
            {#if cache.enabled}
              <Badge variant="success" label="Enabled" />
            {:else}
              <span class="text-xs font-bold text-muted-foreground">Disabled</span>
            {/if}
            {#if cache.writebackHalted}
              <Badge variant="error" label="Writeback halted" />
            {/if}
          {/if}
        </div>
        <dl class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Hits</dt>
            <dd class="text-lg font-semibold dark:text-white">{cache.hits.toLocaleString()}</dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Misses</dt>
            <dd class="text-lg font-semibold dark:text-white">{cache.misses.toLocaleString()}</dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Hit rate</dt>
            <dd class="text-lg font-semibold dark:text-white">
              {hitRate(cache.hits, cache.misses)}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Evictions</dt>
            <dd class="text-lg font-semibold dark:text-white">{cache.evictions.toLocaleString()}</dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Entries</dt>
            <dd class="text-lg font-semibold dark:text-white">{cache.entries.toLocaleString()}</dd>
          </div>
          {#if isObjectDiskCache(cache)}
            <div>
              <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Size</dt>
              <dd class="text-lg font-semibold dark:text-white">
                {formatBytes(cache.sizeBytes)}
                {#if cache.maxSizeBytes > 0}
                  <span class="text-sm font-normal text-neutral-500">
                    / {formatBytes(cache.maxSizeBytes)}
                  </span>
                {/if}
              </dd>
            </div>
            <div>
              <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Dirty objects</dt>
              <dd class="text-lg font-semibold dark:text-white">{cache.dirtyObjects.toLocaleString()}</dd>
            </div>
            <div>
              <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Dirty bytes</dt>
              <dd class="text-lg font-semibold dark:text-white">{formatBytes(cache.dirtyBytes)}</dd>
            </div>
          {/if}
        </dl>
      </section>
    {/each}

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <Package class="size-5 text-coollabs dark:text-warning" />
        <h2 class="text-lg font-bold dark:text-white">Storage operations</h2>
      </div>
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

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <Table2 class="size-5 text-coollabs dark:text-warning" />
        <h2 class="text-lg font-bold dark:text-white">Metadata operations</h2>
      </div>
      {#if data.metadataOps.length === 0}
        <p class="text-sm text-neutral-500">No metadata operations recorded yet.</p>
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
            {#each data.metadataOps as op}
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
        <div class="mb-3 flex items-center gap-2">
          <Cpu class="size-5 text-coollabs dark:text-warning" />
          <h2 class="text-lg font-bold dark:text-white">Process</h2>
        </div>
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
