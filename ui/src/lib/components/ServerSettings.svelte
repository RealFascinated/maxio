<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import type { OrphanMetaEntry } from '$lib/api/maintenance'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { Badge } from '$lib/components/ui/badge'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import RefreshCw from 'lucide-svelte/icons/refresh-cw'
  import { toast, toastApiError } from '$lib/toast'
  import { maintenanceKeys } from '$lib/api/keys'
  import { repairOrphanMeta, scanOrphanMeta } from '$lib/api/maintenance'
  let showRepairConfirm = $state(false)
  let hasScanned = $state(false)

  const scanQuery = createQuery(() => ({
    queryKey: maintenanceKeys.orphanMeta(),
    queryFn: scanOrphanMeta,
    retry: false,
    enabled: false,
  }))

  async function runScan() {
    const result = await scanQuery.refetch()
    if (!result.isError) hasScanned = true
  }

  const repairMutation = createMutation(() => ({
    mutationFn: repairOrphanMeta,
    onSuccess: async (data) => {
      toast.success(
        data.removed === 0
          ? 'No orphaned metadata to remove'
          : `Removed ${data.removed} orphaned metadata rows`,
      )
      const result = await scanQuery.refetch()
      if (!result.isError) hasScanned = true
    },
    onError: (error) => {
      console.error('repairOrphanMeta failed:', error)
      toastApiError(error, 'Repair failed')
    },
  }))

  function formatObjectRef(entry: { bucket: string; key: string; versionId?: string }) {
    if (entry.versionId) {
      return `${entry.bucket}/${entry.key}?versionId=${entry.versionId}`
    }
    return `${entry.bucket}/${entry.key}`
  }

  const orphans = $derived(scanQuery.data?.orphans ?? [])

  const columnHelper = createColumnHelper<typeof sortableTableFeatures, OrphanMetaEntry>()
  const columns = [
    columnHelper.accessor((entry) => formatObjectRef(entry), {
      id: 'object',
      header: sortableHeader('Object'),
    }),
  ]

  const table = createTable({
    features: sortableTableFeatures,
    columns: columns as never,
    get data() {
      return orphans
    },
  })
</script>

<div class="flex flex-col gap-3">
  <PageHeader title="Settings" />

  <section class="flex flex-col gap-2">
    <h2>Orphaned metadata</h2>

    <div class="flex flex-wrap items-center gap-2">
      <Button variant="secondary" loading={scanQuery.isFetching} onclick={runScan}>
        <RefreshCw />
        Scan
      </Button>
      {#if (scanQuery.data?.count ?? 0) > 0}
        <Button
          variant="destructive"
          disabled={scanQuery.isFetching || repairMutation.isPending}
          onclick={() => (showRepairConfirm = true)}
        >
          Repair
        </Button>
      {/if}
      {#if hasScanned && scanQuery.data}
        {#if scanQuery.data.count === 0}
          <Badge variant="success" label="No orphans found" />
        {:else}
          <span class="num text-xs text-muted-foreground">{scanQuery.data.count} orphaned rows</span>
        {/if}
      {/if}
    </div>

    {#if hasScanned && scanQuery.isError}
      <Callout type="danger">
        Scan failed{#if scanQuery.error instanceof Error}: {scanQuery.error.message}{/if}
      </Callout>
    {:else if hasScanned && scanQuery.data && scanQuery.data.count > 0}
      <Table.Root>
        <Table.Header>
          {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row class="hover:bg-transparent">
              {#each headerGroup.headers as header (header.id)}
                <Table.Head>
                  {#if !header.isPlaceholder}
                    <FlexRender header={header} />
                  {/if}
                </Table.Head>
              {/each}
            </Table.Row>
          {/each}
        </Table.Header>
        <Table.Body>
          {#each table.getRowModel().rows as row (row.id)}
            <Table.Row>
              <Table.Cell class="font-mono text-xs">{formatObjectRef(row.original)}</Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    {/if}
  </section>
</div>

<ConfirmDialog
  bind:open={showRepairConfirm}
  title="Repair orphaned metadata"
  description={`Deletes ${scanQuery.data?.count ?? 0} metadata rows with missing object bytes.`}
  confirmLabel="Repair"
  confirmVariant="destructive"
  loading={repairMutation.isPending}
  onConfirm={async () => {
    await repairMutation.mutateAsync()
    showRepairConfirm = false
  }}
/>
