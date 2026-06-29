<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import type { OrphanMetaEntry } from '$lib/api/maintenance'
  import * as Card from '$lib/components/ui/card'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import Settings from 'lucide-svelte/icons/settings'
  import CircleCheck from 'lucide-svelte/icons/circle-check'
  import RefreshCw from 'lucide-svelte/icons/refresh-cw'
  import Trash2 from 'lucide-svelte/icons/trash-2'
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
          : `Removed ${data.removed} orphaned metadata row(s)`,
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

<div class="mx-auto max-w-6xl space-y-6">
  <div class="flex items-center gap-3">
    <Settings class="size-6 text-coollabs dark:text-warning" />
    <h1>Settings</h1>
  </div>

  <Card.Root>
    <Card.Header>
      <Card.Title class="dark:text-white">Orphaned metadata</Card.Title>
      <Card.Description>
        Metadata rows whose object bytes are missing on disk. This can happen after a crash while
        <code class="text-sm">MAXIO_ASYNC_META_WRITE</code> is enabled — bytes are written first and metadata is committed in the background.
      </Card.Description>
    </Card.Header>
    <Card.Content class="space-y-4">
      {#if hasScanned && scanQuery.data?.asyncMetaWrite}
        <Callout type="warning">
          Async metadata writes are enabled on this server. A crash during a PUT can leave metadata
          without matching object bytes until repaired here.
        </Callout>
      {/if}

      <div class="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          disabled={scanQuery.isFetching}
          onclick={runScan}
        >
          <RefreshCw class="size-4 {scanQuery.isFetching ? 'animate-spin' : ''}" />
          Scan
        </Button>
        <Button
          variant="destructive"
          disabled={!hasScanned || scanQuery.isFetching || repairMutation.isPending || (scanQuery.data?.count ?? 0) === 0}
          onclick={() => (showRepairConfirm = true)}
        >
          <Trash2 class="size-4" />
          Repair
        </Button>
        {#if hasScanned && scanQuery.data}
          {#if scanQuery.data.count === 0}
            <span class="inline-flex items-center gap-1.5 text-sm text-success">
              <CircleCheck class="size-4" />
              No orphans found
            </span>
          {:else}
            <span class="text-sm text-neutral-500">
              {scanQuery.data.count} orphaned row(s)
            </span>
          {/if}
        {/if}
      </div>

      {#if hasScanned && scanQuery.isError}
        <Callout type="danger">
          Failed to scan orphaned metadata. {#if scanQuery.error instanceof Error}{scanQuery.error.message}{/if}
        </Callout>
      {:else if hasScanned && scanQuery.data && scanQuery.data.count > 0}
        <div class="overflow-hidden rounded-sm border-2 border-neutral-200 dark:border-coolgray-200">
          <Table.Root>
            <Table.Header>
              {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
                <Table.Row>
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
                  <Table.Cell class="font-mono text-sm">{formatObjectRef(row.original)}</Table.Cell>
                </Table.Row>
              {/each}
            </Table.Body>
          </Table.Root>
        </div>
      {/if}
    </Card.Content>
  </Card.Root>
</div>

<ConfirmDialog
  bind:open={showRepairConfirm}
  title="Repair orphaned metadata?"
  description="This deletes metadata rows whose object bytes are missing on disk. Object listings will no longer show these entries."
  confirmLabel="Repair"
  confirmVariant="destructive"
  loading={repairMutation.isPending}
  onConfirm={async () => {
    await repairMutation.mutateAsync()
    showRepairConfirm = false
  }}
/>
