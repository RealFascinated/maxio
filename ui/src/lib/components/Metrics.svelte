<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import * as Card from '$lib/components/ui/card'
  import { Callout } from '$lib/components/ui/callout'
  import { Badge } from '$lib/components/ui/badge'
  import SemiGauge from '$lib/components/metrics/SemiGauge.svelte'
  import BarChart2 from 'lucide-svelte/icons/bar-chart-2'
  import Cpu from 'lucide-svelte/icons/cpu'
  import Database from 'lucide-svelte/icons/database'
  import ChevronDown from 'lucide-svelte/icons/chevron-down'
  import HardDrive from 'lucide-svelte/icons/hard-drive'
  import Package from 'lucide-svelte/icons/package'
  import Table2 from 'lucide-svelte/icons/table-2'
  import Users from 'lucide-svelte/icons/users'
  import { metricsKeys } from '$lib/api/keys'
  import { fetchMetrics, type CacheSnapshot } from '$lib/api/metrics'
  import { formatBytes, formatThroughput } from '$lib/format-bytes'
  import { cn } from '$lib/utils.js'
  import { formatIops, formatLatency, formatMetricName, formatUptime, hitRate } from '$lib/format'

  const metricsQuery = createQuery(() => ({
    queryKey: metricsKeys.snapshot(),
    queryFn: fetchMetrics,
    refetchInterval: 1_000,
  }))

  let peakThroughput = $state(0)
  let peakIops = $state(0)
  let showCaches = $state(false)

  function isObjectDiskCache(cache: CacheSnapshot): boolean {
    return cache.id === 'object_disk'
  }

  $effect(() => {
    const data = metricsQuery.data
    if (!data) return
    const total = data.throughput.readBytesPerSec + data.throughput.writeBytesPerSec
    peakThroughput = Math.max(total, peakThroughput * 0.95)
    const totalIops =
      data.opsTotals.readIops + data.opsTotals.writeIops + data.opsTotals.metaIops
    peakIops = Math.max(totalIops, peakIops * 0.95)
  })
</script>

<div class="mx-auto max-w-6xl space-y-6">
  <div class="flex items-center justify-between gap-4">
    <div class="flex items-center gap-3">
      <BarChart2 class="size-6 text-coollabs dark:text-warning" />
      <h1 class="text-2xl font-bold dark:text-white">Metrics</h1>
    </div>
    {#if metricsQuery.data}
      <div
        class="flex items-center gap-2 rounded-sm border-2 border-neutral-200 bg-white px-3 py-1.5 text-sm dark:border-coolgray-200 dark:bg-coolgray-100"
      >
        <Users class="size-4 text-neutral-500" />
        <span class="text-neutral-500">Active S3 Connections</span>
        <span class="font-semibold tabular-nums dark:text-white">
          {metricsQuery.data.activeClients.toLocaleString()}
        </span>
      </div>
    {/if}
  </div>

  {#if metricsQuery.isPending}
    <p class="text-sm text-neutral-500">Loading metrics…</p>
  {:else if metricsQuery.isError}
    <Callout type="danger">
      Failed to load metrics. {#if metricsQuery.error instanceof Error}{metricsQuery.error.message}{/if}
    </Callout>
  {:else if metricsQuery.data}
    {@const data = metricsQuery.data}
    {@const readThroughput = formatThroughput(data.throughput.readBytesPerSec)}
    {@const writeThroughput = formatThroughput(data.throughput.writeBytesPerSec)}
    {@const totalThroughputBps =
      data.throughput.readBytesPerSec + data.throughput.writeBytesPerSec}
    {@const totalThroughput = formatThroughput(totalThroughputBps)}
    {@const opsTotal =
      data.opsTotals.readIops + data.opsTotals.writeIops + data.opsTotals.metaIops}
    {@const readLatency = formatLatency(data.latency.readSeconds)}
    {@const writeLatency = formatLatency(data.latency.writeSeconds)}

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <Database class="size-5 text-coollabs dark:text-warning" />
        <h2 class="text-lg font-bold dark:text-white">Storage</h2>
      </div>
      <dl class="grid gap-4 sm:grid-cols-4">
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
        <div>
          <dt class="text-xs font-medium uppercase tracking-wide text-neutral-500">Uptime</dt>
          <dd class="text-lg font-semibold dark:text-white">{formatUptime(data.uptimeSeconds)}</dd>
        </div>
      </dl>
    </section>

    <div class="space-y-4">
      <Card.Root class="flex h-full flex-col gap-0 border-2 border-neutral-200 bg-white py-0 dark:border-coolgray-200 dark:bg-coolgray-100">
        <Card.Header class="border-b border-neutral-200 pt-4 !pb-4 dark:border-coolgray-200">
          <Card.Title class="text-center text-base font-bold dark:text-white">
            R/W Throughput
          </Card.Title>
        </Card.Header>
        <Card.Content class="flex flex-1 flex-col py-5">
          <div class="grid items-center gap-6 md:grid-cols-3">
            <div class="text-center md:text-left">
              <div class="mb-1 flex items-center justify-center gap-2 md:justify-start">
                <span class="size-2 rounded-full bg-coollabs"></span>
                <span class="text-sm font-medium text-neutral-500">Read</span>
              </div>
              <p class="text-3xl font-bold tabular-nums dark:text-white">{readThroughput.value}</p>
              <p class="text-sm text-neutral-500">{readThroughput.unit}</p>
            </div>

            <SemiGauge
              segments={[
                { value: data.throughput.readBytesPerSec, class: 'stroke-coollabs' },
                { value: data.throughput.writeBytesPerSec, class: 'stroke-pink-500' },
              ]}
              total={Math.max(totalThroughputBps, peakThroughput, 1)}
              centerLabel="Total"
              centerValue={totalThroughput.value}
              centerUnit={totalThroughput.unit}
            />

            <div class="text-center md:text-right">
              <div class="mb-1 flex items-center justify-center gap-2 md:justify-end">
                <span class="size-2 rounded-full bg-pink-500"></span>
                <span class="text-sm font-medium text-neutral-500">Write</span>
              </div>
              <p class="text-3xl font-bold tabular-nums dark:text-white">{writeThroughput.value}</p>
              <p class="text-sm text-neutral-500">{writeThroughput.unit}</p>
            </div>
          </div>
          <p class="mt-auto pt-4 text-center text-xs text-neutral-500">
            Rolling average over the last {data.throughput.windowSeconds}s
          </p>
        </Card.Content>
      </Card.Root>

      <div class="grid items-stretch gap-4 lg:grid-cols-2">
        <Card.Root class="flex h-full flex-col gap-0 border-2 border-neutral-200 bg-white py-0 dark:border-coolgray-200 dark:bg-coolgray-100">
          <Card.Header class="border-b border-neutral-200 pt-4 !pb-4 dark:border-coolgray-200">
            <Card.Title class="text-center text-base font-bold dark:text-white">Latency</Card.Title>
          </Card.Header>
          <Card.Content class="flex flex-1 flex-col py-5">
            <div class="grid gap-6 sm:grid-cols-2">
              <div class="text-center">
                <div class="mb-1 flex items-center justify-center gap-2">
                  <span class="size-2 rounded-full bg-coollabs"></span>
                  <span class="text-sm font-medium text-neutral-500">Read</span>
                </div>
                <p class="text-3xl font-bold tabular-nums dark:text-white">
                  {readLatency.value}
                </p>
                {#if readLatency.unit}
                  <p class="text-sm text-neutral-500">{readLatency.unit}</p>
                {/if}
              </div>
              <div class="text-center">
                <div class="mb-1 flex items-center justify-center gap-2">
                  <span class="size-2 rounded-full bg-pink-500"></span>
                  <span class="text-sm font-medium text-neutral-500">Write</span>
                </div>
                <p class="text-3xl font-bold tabular-nums dark:text-white">
                  {writeLatency.value}
                </p>
                {#if writeLatency.unit}
                  <p class="text-sm text-neutral-500">{writeLatency.unit}</p>
                {/if}
              </div>
            </div>
            <p class="mt-auto pt-4 text-center text-xs text-neutral-500">
              Avg over the last {data.latency.windowSeconds}s, end-to-end
            </p>
          </Card.Content>
        </Card.Root>

        <Card.Root class="flex h-full flex-col gap-0 border-2 border-neutral-200 bg-white py-0 dark:border-coolgray-200 dark:bg-coolgray-100">
          <Card.Header class="border-b border-neutral-200 pt-4 !pb-4 dark:border-coolgray-200">
            <Card.Title class="text-center text-base font-bold dark:text-white">IOPS</Card.Title>
          </Card.Header>
          <Card.Content class="flex flex-1 flex-col py-5">
            <div class="flex flex-col items-center gap-4 sm:flex-row sm:justify-center">
              <SemiGauge
                segments={[
                  { value: data.opsTotals.readIops, class: 'stroke-coollabs' },
                  { value: data.opsTotals.writeIops, class: 'stroke-pink-500' },
                  { value: data.opsTotals.metaIops, class: 'stroke-sky-400' },
                ]}
                total={Math.max(opsTotal, peakIops, 1)}
                centerValue={formatIops(opsTotal)}
                centerUnit="IOPS"
              />
              <ul class="space-y-2 text-sm">
                <li class="flex items-center gap-2">
                  <span class="size-2 rounded-full bg-coollabs"></span>
                  <span class="text-neutral-500">Read</span>
                  <span class="ml-auto font-semibold tabular-nums dark:text-white">
                    {formatIops(data.opsTotals.readIops)}
                  </span>
                </li>
                <li class="flex items-center gap-2">
                  <span class="size-2 rounded-full bg-pink-500"></span>
                  <span class="text-neutral-500">Write</span>
                  <span class="ml-auto font-semibold tabular-nums dark:text-white">
                    {formatIops(data.opsTotals.writeIops)}
                  </span>
                </li>
                <li class="flex items-center gap-2">
                  <span class="size-2 rounded-full bg-sky-400"></span>
                  <span class="text-neutral-500">Meta</span>
                  <span class="ml-auto font-semibold tabular-nums dark:text-white">
                    {formatIops(data.opsTotals.metaIops)}
                  </span>
                </li>
              </ul>
            </div>
            <p class="mt-auto pt-4 text-center text-xs text-neutral-500">
              Rolling average over the last {data.opsTotals.windowSeconds}s
            </p>
          </Card.Content>
        </Card.Root>
      </div>
    </div>

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <button
        type="button"
        class="flex w-full items-center justify-between gap-2 text-left"
        onclick={() => (showCaches = !showCaches)}
        aria-expanded={showCaches}
      >
        <span class="flex items-center gap-2">
          <HardDrive class="size-5 shrink-0 text-coollabs dark:text-warning" />
          <span class="text-lg font-bold dark:text-white">
            {showCaches ? 'Hide caches' : 'Show caches'}
          </span>
          <span class="text-xs font-medium text-neutral-500">({data.caches.length})</span>
        </span>
        <ChevronDown
          class={cn('size-5 shrink-0 text-neutral-500 transition-transform', showCaches && 'rotate-180')}
        />
      </button>

      {#if showCaches}
        <div class="mt-4 space-y-4 border-t border-neutral-200 pt-4 dark:border-coolgray-200">
          {#each data.caches as cache (cache.id)}
            <div>
              <div class="mb-3 flex flex-wrap items-center gap-2">
                <h3 class="text-base font-bold dark:text-white">{formatMetricName(cache.id)}</h3>
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
            </div>
          {/each}
        </div>
      {/if}
    </section>

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
                  {#if op.count > 0}
                    {@const lat = formatLatency(op.sumSeconds / op.count)}
                    {lat.value}{lat.unit ? ` ${lat.unit}` : ''}
                  {:else}
                    {@const lat = formatLatency(0)}
                    {lat.value}{lat.unit ? ` ${lat.unit}` : ''}
                  {/if}
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
                  {#if op.count > 0}
                    {@const lat = formatLatency(op.sumSeconds / op.count)}
                    {lat.value}{lat.unit ? ` ${lat.unit}` : ''}
                  {:else}
                    {@const lat = formatLatency(0)}
                    {lat.value}{lat.unit ? ` ${lat.unit}` : ''}
                  {/if}
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
        <dl class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
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
