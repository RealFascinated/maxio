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
  import HardDrive from 'lucide-svelte/icons/hard-drive'
  import Package from 'lucide-svelte/icons/package'
  import Table2 from 'lucide-svelte/icons/table-2'
  import Users from 'lucide-svelte/icons/users'
  import { metricsKeys } from '$lib/api/keys'
  import { fetchMetrics } from '$lib/api/metrics'
  import { formatBytes, formatThroughput } from '$lib/format-bytes'
  import {
    formatIops,
    formatLatency,
    formatMetricName,
    formatOperationName,
    formatUptime,
    hitRate,
  } from '$lib/format'

  const metricsQuery = createQuery(() => ({
    queryKey: metricsKeys.snapshot(),
    queryFn: fetchMetrics,
    refetchInterval: 1_000,
    staleTime: 0,
    // Polling dashboard: structural sharing keeps the same `data` reference when
    // values are deeply equal, and Svelte won't re-render plain nested objects.
    structuralSharing: false,
  }))

  let peakThroughput = $state(0)
  let peakIops = $state(0)
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
      <h1>Metrics</h1>
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

    {@const objectDiskCache = data.caches.find((cache) => cache.id === 'object_disk')}
    {@const otherCaches = data.caches.filter((cache) => cache.id !== 'object_disk')}

    {#if objectDiskCache}
      {@const sizePercent =
        objectDiskCache.maxSizeBytes > 0
          ? Math.min(100, (objectDiskCache.sizeBytes / objectDiskCache.maxSizeBytes) * 100)
          : 0}
      <Card.Root class="flex flex-col gap-0 border-2 border-neutral-200 bg-white py-0 dark:border-coolgray-200 dark:bg-coolgray-100">
        <Card.Header
          class="flex flex-row flex-wrap items-center justify-between gap-3 border-b border-neutral-200 pt-4 !pb-4 dark:border-coolgray-200"
        >
          <div class="flex items-center gap-2">
            <HardDrive class="size-5 text-coollabs dark:text-warning" />
            <Card.Title class="text-base font-bold dark:text-white">
              {formatMetricName(objectDiskCache.id)}
            </Card.Title>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            {#if objectDiskCache.enabled}
              <Badge variant="success" label="Enabled" />
            {:else}
              <Badge variant="warning" label="Disabled" />
            {/if}
            {#if objectDiskCache.writebackHalted}
              <Badge variant="error" label="Writeback halted" />
            {/if}
          </div>
        </Card.Header>
        <Card.Content class="py-5">
          <div class="grid gap-4 lg:grid-cols-3 lg:items-stretch">
            <div
              class="flex flex-col rounded-sm border-2 border-neutral-200 p-4 dark:border-coolgray-200 dark:bg-coolgray-300"
            >
              <p class="mb-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
                Performance
              </p>
              <dl class="flex flex-1 flex-col gap-3">
                <div class="flex items-baseline justify-between gap-4">
                  <dt class="text-sm text-neutral-500">Hits</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {objectDiskCache.hits.toLocaleString()}
                  </dd>
                </div>
                <div class="flex items-baseline justify-between gap-4">
                  <dt class="text-sm text-neutral-500">Misses</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {objectDiskCache.misses.toLocaleString()}
                  </dd>
                </div>
                <div
                  class="mt-auto flex items-baseline justify-between gap-4 border-t border-neutral-200 pt-3 dark:border-coolgray-200"
                >
                  <dt class="text-sm font-medium text-neutral-500">Hit Rate</dt>
                  <dd class="text-xl font-bold tabular-nums text-coollabs dark:text-warning">
                    {hitRate(objectDiskCache.hits, objectDiskCache.misses)}
                  </dd>
                </div>
              </dl>
            </div>

            <div
              class="flex flex-col rounded-sm border-2 border-neutral-200 p-4 dark:border-coolgray-200 dark:bg-coolgray-300"
            >
              <p class="mb-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
                Capacity
              </p>
              <dl class="flex flex-1 flex-col gap-3">
                <div class="flex items-baseline justify-between gap-4">
                  <dt class="text-sm text-neutral-500">Entries</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {objectDiskCache.entries.toLocaleString()}
                  </dd>
                </div>
                <div class="flex items-baseline justify-between gap-4">
                  <dt class="text-sm text-neutral-500">Evictions</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {objectDiskCache.evictions.toLocaleString()}
                  </dd>
                </div>
                <div class="mt-auto border-t border-neutral-200 pt-3 dark:border-coolgray-200">
                  <div class="flex items-baseline justify-between gap-4">
                    <dt class="text-sm font-medium text-neutral-500">Size</dt>
                    <dd class="text-right">
                      <span class="text-xl font-bold tabular-nums dark:text-white">
                        {formatBytes(objectDiskCache.sizeBytes)}
                      </span>
                      {#if objectDiskCache.maxSizeBytes > 0}
                        <span class="text-sm text-neutral-500">
                          / {formatBytes(objectDiskCache.maxSizeBytes)}
                        </span>
                      {/if}
                    </dd>
                  </div>
                  {#if objectDiskCache.maxSizeBytes > 0}
                    <div
                      class="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-200 dark:bg-coolgray-200"
                    >
                      <div
                        class="h-full rounded-full bg-coollabs transition-[width] dark:bg-warning"
                        style="width: {sizePercent}%"
                      ></div>
                    </div>
                  {/if}
                </div>
              </dl>
            </div>

            <div
              class="flex flex-col rounded-sm border-2 border-neutral-200 p-4 dark:border-coolgray-200 dark:bg-coolgray-300"
            >
              <p class="mb-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
                Writeback
              </p>
              <dl class="flex flex-1 flex-col gap-3">
                <div class="flex items-baseline justify-between gap-4">
                  <dt class="text-sm text-neutral-500">Dirty Objects</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {objectDiskCache.dirtyObjects.toLocaleString()}
                  </dd>
                </div>
                <div
                  class="mt-auto flex items-baseline justify-between gap-4 border-t border-neutral-200 pt-3 dark:border-coolgray-200"
                >
                  <dt class="text-sm font-medium text-neutral-500">Dirty Bytes</dt>
                  <dd class="text-xl font-bold tabular-nums dark:text-white">
                    {formatBytes(objectDiskCache.dirtyBytes)}
                  </dd>
                </div>
              </dl>
            </div>
          </div>
        </Card.Content>
      </Card.Root>
    {/if}

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <HardDrive class="size-5 text-coollabs dark:text-warning" />
        <h3 class="text-base font-bold dark:text-white">Caches</h3>
      </div>
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <Table.Head>Cache</Table.Head>
            <Table.Head class="text-right">Hits</Table.Head>
            <Table.Head class="text-right">Misses</Table.Head>
            <Table.Head class="text-right">Hit Rate</Table.Head>
            <Table.Head class="text-right">Evictions</Table.Head>
            <Table.Head class="text-right">Entries</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each otherCaches as cache (cache.id)}
            <Table.Row>
              <Table.Cell class="text-base font-bold dark:text-white">
                {formatMetricName(cache.id)}
              </Table.Cell>
              <Table.Cell class="text-right">{cache.hits.toLocaleString()}</Table.Cell>
              <Table.Cell class="text-right">{cache.misses.toLocaleString()}</Table.Cell>
              <Table.Cell class="text-right">{hitRate(cache.hits, cache.misses)}</Table.Cell>
              <Table.Cell class="text-right">{cache.evictions.toLocaleString()}</Table.Cell>
              <Table.Cell class="text-right">{cache.entries.toLocaleString()}</Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    </section>

    <section class="rounded-sm border-2 border-neutral-200 bg-white p-4 dark:border-coolgray-200 dark:bg-coolgray-100">
      <div class="mb-3 flex items-center gap-2">
        <Package class="size-5 text-coollabs dark:text-warning" />
        <h3 class="text-base font-bold dark:text-white">Storage Operations</h3>
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
                <Table.Cell class="text-base font-bold dark:text-white">
                  {formatOperationName(op.operation)}
                </Table.Cell>
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
        <h3 class="text-base font-bold dark:text-white">Metadata Operations</h3>
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
                <Table.Cell class="text-base font-bold dark:text-white">
                  {formatOperationName(op.operation)}
                </Table.Cell>
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
          <h3 class="text-base font-bold dark:text-white">Process</h3>
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
