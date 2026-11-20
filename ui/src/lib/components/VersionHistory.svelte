<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import type { Version } from '$lib/api/versions'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Tag } from '$lib/components/ui/tag'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import Download from 'lucide-svelte/icons/download'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import { versionKeys } from '$lib/api/keys'
  import { deleteVersion as deleteVersionApi, listVersions } from '$lib/api/versions'
  import { ApiError, encodeObjectKey } from '$lib/api/http'
  import { isSessionExpiredError } from '$lib/api/session'
  import { queryClient } from '$lib/query/client'
  import { formatBytes } from '$lib/format-bytes'

  interface Props {
    bucket: string
    objectKey: string
    onVersionDeleted?: () => void
  }
  let { bucket, objectKey, onVersionDeleted }: Props = $props()

  const versionsQuery = createQuery(() => ({
    queryKey: versionKeys.list(bucket, objectKey),
    queryFn: () => listVersions(bucket, objectKey),
  }))
  const deleteVersionMutation = createMutation(() => ({
    mutationFn: (versionId: string) => deleteVersionApi(bucket, objectKey, versionId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: versionKeys.list(bucket, objectKey) })
      onVersionDeleted?.()
    },
  }))

  const versions = $derived(versionsQuery.data?.versions ?? [])
  let deleteError = $state<string | null>(null)
  let versionToDelete = $state<string | null>(null)

  const columnHelper = createColumnHelper<typeof sortableTableFeatures, Version>()
  const columns = [
    columnHelper.accessor('versionId', { header: sortableHeader('Version') }),
    columnHelper.accessor('lastModified', { header: sortableHeader('Modified') }),
    columnHelper.accessor('size', { header: sortableHeader('Size', 'ml-auto') }),
    columnHelper.display({ id: 'actions', enableSorting: false, header: '' }),
  ]

  const table = createTable({
    features: sortableTableFeatures,
    columns: columns as never,
    get data() {
      return versions
    },
  })

  function formatVersionDate(iso: string): string {
    return new Date(iso).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' })
  }

  async function confirmDeleteVersion() {
    if (!versionToDelete) return
    try {
      deleteError = null
      await deleteVersionMutation.mutateAsync(versionToDelete)
      versionToDelete = null
    } catch (err) {
      if (isSessionExpiredError(err)) return
      console.error('deleteVersion failed:', err)
      deleteError = err instanceof ApiError ? err.message : 'Failed to connect to server'
    }
  }

  function downloadVersion(versionId: string) {
    window.open(
      `/api/buckets/${encodeURIComponent(bucket)}/objects/${encodeObjectKey(objectKey)}?versionId=${encodeURIComponent(versionId)}&download=1`,
      '_blank'
    )
  }
</script>

{#if versionsQuery.isError || deleteError}
  <Callout type="danger" class="mb-2">
    {deleteError ?? (versionsQuery.error instanceof ApiError ? versionsQuery.error.message : 'Failed to load versions')}
  </Callout>
{/if}

{#if versionsQuery.isPending}
  <div class="flex flex-col gap-2" aria-busy="true">
    {#each { length: 3 } as _, i (i)}
      <Skeleton class="h-6 w-full" />
    {/each}
  </div>
{:else if versions.length === 0}
  <EmptyState title="No versions" class="py-6" />
{:else}
  <div class="[&_[data-slot=table-container]]:rounded-none [&_[data-slot=table-container]]:border-0 [&_[data-slot=table-container]]:bg-transparent">
  <Table.Root class="text-xs">
    <Table.Header>
      {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
        <Table.Row>
          {#each headerGroup.headers as header (header.id)}
            <Table.Head
              class={header.column.id === 'actions'
                ? 'w-14 px-1'
                : header.column.id === 'size'
                  ? 'px-1.5 text-right'
                  : 'px-1.5'}
            >
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
        {@const version = row.original}
        <Table.Row class={version.isDeleteMarker ? 'text-muted-foreground' : ''}>
          <Table.Cell class="px-1.5 font-mono">
            <span class="inline-flex items-center gap-1.5">
              {#if version.versionId}
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <span {...props}>…{version.versionId!.slice(-8)}</span>
                    {/snippet}
                  </Tooltip.Trigger>
                  <Tooltip.Content class="font-mono">{version.versionId}</Tooltip.Content>
                </Tooltip.Root>
              {:else}
                <span>null</span>
              {/if}
              {#if row.index === 0}<Tag>latest</Tag>{/if}
              {#if version.isDeleteMarker}<Tag>delete marker</Tag>{/if}
            </span>
          </Table.Cell>
          <Table.Cell class="num px-1.5 text-muted-foreground">{formatVersionDate(version.lastModified)}</Table.Cell>
          <Table.Cell class="num px-1.5 text-right text-muted-foreground">
            {version.isDeleteMarker ? '' : formatBytes(version.size)}
          </Table.Cell>
          <Table.Cell class="px-1">
            <div class="flex items-center justify-end">
              {#if !version.isDeleteMarker && version.versionId}
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="ghost"
                        size="icon-xs"
                        aria-label="Download version"
                        onclick={() => downloadVersion(version.versionId!)}
                      >
                        <Download />
                      </Button>
                    {/snippet}
                  </Tooltip.Trigger>
                  <Tooltip.Content>Download version</Tooltip.Content>
                </Tooltip.Root>
              {/if}
              {#if version.versionId}
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="ghost"
                        size="icon-xs"
                        aria-label="Delete version"
                        onclick={() => (versionToDelete = version.versionId)}
                      >
                        <Trash2 />
                      </Button>
                    {/snippet}
                  </Tooltip.Trigger>
                  <Tooltip.Content>Delete version</Tooltip.Content>
                </Tooltip.Root>
              {/if}
            </div>
          </Table.Cell>
        </Table.Row>
      {/each}
    </Table.Body>
  </Table.Root>
  </div>
{/if}

<ConfirmDialog
  open={versionToDelete !== null}
  title="Delete version permanently?"
  description="This cannot be undone."
  confirmLabel="Delete version"
  confirmVariant="destructive"
  confirmationText="delete"
  loading={deleteVersionMutation.isPending}
  onClose={() => (versionToDelete = null)}
  onConfirm={confirmDeleteVersion}
/>
