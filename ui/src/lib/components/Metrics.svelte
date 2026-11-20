<script lang="ts">
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { createQuery } from '@tanstack/svelte-query'
  import { ApiError } from '$lib/api/http'
  import { metricsKeys } from '$lib/api/keys'
  import {
    fetchMetrics,
    fetchMetricsHistory,
    METRICS_RANGES,
    type MetricsRange,
  } from '$lib/api/metrics'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { queryClient } from '$lib/query/client'
  import CacheSection from './metrics/CacheSection.svelte'
  import MetricsHeader from './metrics/MetricsHeader.svelte'
  import OperationsSection from './metrics/OperationsSection.svelte'
  import OverviewSection from './metrics/OverviewSection.svelte'
  import SectionTabs from './metrics/SectionTabs.svelte'
  import StorageSection from './metrics/StorageSection.svelte'
  import SystemSection from './metrics/SystemSection.svelte'
  import TrafficSection from './metrics/TrafficSection.svelte'
  import { SECTIONS, type HistoryStatus, type MetricsView, type SectionId } from './metrics/metrics-view'

  const DEFAULT_RANGE: MetricsRange = '1h'

  const range = $derived(
    METRICS_RANGES.find((value) => value === page.url.searchParams.get('range')) ?? DEFAULT_RANGE,
  )
  const section = $derived(
    SECTIONS.find((entry) => entry.id === page.url.searchParams.get('section'))?.id ?? 'overview',
  )
  let refreshSeconds = $state('10')
  const refetchInterval = $derived(Number(refreshSeconds) > 0 ? Number(refreshSeconds) * 1000 : false)

  function setParam(name: string, value: string) {
    const url = new URL(page.url)
    url.searchParams.set(name, value)
    goto(`${url.pathname}${url.search}`, { replaceState: true, keepFocus: true, noScroll: true })
  }

  const snapshotQuery = createQuery(() => ({
    queryKey: metricsKeys.snapshot(),
    queryFn: fetchMetrics,
    refetchInterval,
    staleTime: 0,
  }))

  const historyQuery = createQuery(() => ({
    queryKey: metricsKeys.history(range),
    queryFn: () => fetchMetricsHistory(range),
    refetchInterval,
    staleTime: 0,
    placeholderData: (previous) => previous,
    retry: (count, error) => !(error instanceof ApiError && error.status === 403) && count < 2,
  }))

  const historyStatus = $derived<HistoryStatus>(
    historyQuery.isError
      ? historyQuery.error instanceof ApiError && historyQuery.error.status === 403
        ? 'forbidden'
        : 'error'
      : historyQuery.isPending
        ? 'loading'
        : 'ready',
  )

  const view = $derived<MetricsView | null>(
    snapshotQuery.data
      ? { snapshot: snapshotQuery.data, history: historyQuery.data, historyStatus, range }
      : null,
  )

  function reload() {
    void queryClient.invalidateQueries({ queryKey: metricsKeys.all })
  }

  const selectSection = (id: SectionId) => setParam('section', id)
</script>

<div class="mx-auto flex max-w-7xl flex-col gap-3">
  <MetricsHeader
    {range}
    onrange={(value) => setParam('range', value)}
    {refreshSeconds}
    onrefresh={(value) => (refreshSeconds = value)}
    lastUpdated={Math.max(snapshotQuery.dataUpdatedAt, historyQuery.dataUpdatedAt)}
    fetching={snapshotQuery.isFetching || historyQuery.isFetching}
    onreload={reload}
  />
  <SectionTabs value={section} onchange={selectSection} />

  {#if historyStatus === 'forbidden'}
    <Callout type="info">History requires the root account</Callout>
  {:else if historyStatus === 'error'}
    <Callout type="danger">
      <div class="flex items-center justify-between gap-3">
        {historyQuery.error instanceof Error ? historyQuery.error.message : 'Failed to load history'}
        <Button variant="outline" size="xs" onclick={() => historyQuery.refetch()}>Retry</Button>
      </div>
    </Callout>
  {/if}

  {#if snapshotQuery.isError}
    <Callout type="danger">
      <div class="flex items-center justify-between gap-3">
        {snapshotQuery.error instanceof Error ? snapshotQuery.error.message : 'Failed to load metrics'}
        <Button variant="outline" size="xs" onclick={() => snapshotQuery.refetch()}>Retry</Button>
      </div>
    </Callout>
  {:else if !view}
    <div class="grid grid-cols-2 gap-2 lg:grid-cols-4" role="status" aria-label="Loading metrics">
      {#each Array(8) as _, index (index)}
        <Skeleton class="h-20 rounded-surface" />
      {/each}
    </div>
  {:else if section === 'overview'}
    <OverviewSection {view} onselect={selectSection} />
  {:else if section === 'traffic'}
    <TrafficSection {view} />
  {:else if section === 'storage'}
    <StorageSection {view} />
  {:else if section === 'cache'}
    <CacheSection {view} />
  {:else if section === 'operations'}
    <OperationsSection {view} />
  {:else}
    <SystemSection {view} />
  {/if}
</div>
