<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import type { Bucket } from '$lib/api/buckets'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Badge } from '$lib/components/ui/badge'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import MoreHorizontal from 'lucide-svelte/icons/ellipsis'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Settings from 'lucide-svelte/icons/settings'
  import { toast, toastApiError } from '$lib/toast'
  import { bucketKeys } from '$lib/api/keys'
  import { createBucket as createBucketApi, deleteBucket as deleteBucketApi, listBuckets } from '$lib/api/buckets'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { formatBytes } from '$lib/format-bytes'
  import { formatDate } from '$lib/format'
  import { routes } from '$lib/navigation'

  interface Props {
    canCreateBucket?: boolean
  }
  let { canCreateBucket = true }: Props = $props()

  let showCreate = $state(false)
  let newBucketName = $state('')
  let bucketToDelete = $state<string | null>(null)
  let createBucketInput = $state<HTMLInputElement | null>(null)
  let searchInput = $state('')

  $effect(() => {
    if (showCreate && createBucketInput) {
      setTimeout(() => createBucketInput?.focus(), 0)
    }
  })

  $effect(() => {
    if (!page.url.searchParams.has('new')) return
    if (canCreateBucket) showCreate = true
    goto(routes.home(), { replace: true, reset: false })
  })

  const bucketNameRules = [
    { valid: (name: string) => name.length >= 3 && name.length <= 63, message: 'Must be 3–63 characters' },
    { valid: (name: string) => /^[a-z0-9.-]+$/.test(name), message: 'Lowercase letters, digits, hyphens and dots only' },
    { valid: (name: string) => /^[a-z0-9]/.test(name) && /[a-z0-9]$/.test(name), message: 'Must start and end with a letter or digit' },
    { valid: (name: string) => !name.includes('..'), message: 'Must not contain consecutive dots' },
    { valid: (name: string) => !/^\d{1,3}(\.\d{1,3}){3}$/.test(name), message: 'Must not be formatted as an IP address' },
  ]

  const createNameError = $derived.by(() => {
    const name = newBucketName.trim()
    if (!name) return null
    return bucketNameRules.find((rule) => !rule.valid(name))?.message ?? null
  })

  const bucketsQuery = createQuery(() => ({
    queryKey: bucketKeys.list(),
    queryFn: listBuckets,
  }))

  const allBuckets = $derived(bucketsQuery.data?.buckets ?? [])
  const filteredBuckets = $derived.by(() => {
    const q = searchInput.trim().toLowerCase()
    if (!q) return allBuckets
    return allBuckets.filter((bucket) => bucket.name.toLowerCase().includes(q))
  })

  function savedBytes(bucket: Bucket): number | null {
    if (bucket.sizeBytes === null || bucket.storedBytes === null) return null
    return Math.max(0, bucket.sizeBytes - bucket.storedBytes)
  }

  const columnHelper = createColumnHelper<typeof sortableTableFeatures, Bucket>()
  const columns = [
    columnHelper.accessor('name', { header: sortableHeader('Name') }),
    columnHelper.accessor('objectCount', {
      header: sortableHeader('Objects'),
      sortUndefined: 'last',
    }),
    columnHelper.accessor('sizeBytes', {
      header: sortableHeader('Size'),
      sortUndefined: 'last',
    }),
    columnHelper.accessor('storedBytes', {
      header: sortableHeader('On disk'),
      sortUndefined: 'last',
    }),
    columnHelper.accessor(savedBytes, {
      id: 'savedBytes',
      header: sortableHeader('Saved'),
      sortUndefined: 'last',
    }),
    columnHelper.accessor('createdAt', { header: sortableHeader('Created') }),
    columnHelper.accessor('publicRead', { header: sortableHeader('Access') }),
    columnHelper.display({ id: 'actions', enableSorting: false, header: '' }),
  ]

  const table = createTable({
    features: sortableTableFeatures,
    columns: columns as never,
    get data() {
      return filteredBuckets
    },
  })

  const rightAligned = new Set(['objectCount', 'sizeBytes', 'storedBytes', 'savedBytes'])

  const createBucketMutation = createMutation(() => ({
    mutationFn: createBucketApi,
    onSuccess: (_data, name) => {
      toast.success(`Bucket ${name} created`)
      newBucketName = ''
      showCreate = false
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  const deleteBucketMutation = createMutation(() => ({
    mutationFn: deleteBucketApi,
    onSuccess: (_data, name) => {
      toast.success(`Bucket ${name} deleted`)
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  function closeCreate() {
    showCreate = false
    newBucketName = ''
  }

  async function createBucket() {
    const name = newBucketName.trim()
    if (!name || createNameError) return
    try {
      await createBucketMutation.mutateAsync(name)
    } catch (err) {
      console.error('createBucket failed:', err)
      toastApiError(err, 'Connection failed')
    }
  }

  async function confirmDeleteBucket() {
    if (!bucketToDelete) return
    const name = bucketToDelete
    try {
      await deleteBucketMutation.mutateAsync(name)
      bucketToDelete = null
    } catch (err) {
      console.error('deleteBucket failed:', err)
      toastApiError(err, 'Connection failed')
    }
  }
</script>

<div class="flex flex-col gap-3">
  <PageHeader title="Buckets" />

  {#if bucketsQuery.isError}
    <Callout type="danger">
      {bucketsQuery.error instanceof ApiError ? bucketsQuery.error.message : 'Failed to load buckets'}
    </Callout>
  {:else if !bucketsQuery.isPending && allBuckets.length === 0}
    <div class="rounded-surface border border-border bg-panel">
      <EmptyState title="No buckets">
        {#snippet action()}
          {#if canCreateBucket}
            <Button size="sm" onclick={() => (showCreate = true)}><Plus />Create bucket</Button>
          {/if}
        {/snippet}
      </EmptyState>
    </div>
  {:else}
    <Toolbar>
      <Input
        placeholder="Filter"
        class="w-56 max-w-full"
        bind:value={searchInput}
        aria-label="Filter buckets"
      />
      {#snippet end()}
        {#if !bucketsQuery.isPending}
          <span class="num text-xs text-muted-foreground">
            {filteredBuckets.length === allBuckets.length
              ? allBuckets.length
              : `${filteredBuckets.length} of ${allBuckets.length}`}
            {allBuckets.length === 1 ? 'bucket' : 'buckets'}
          </span>
        {/if}
        {#if canCreateBucket}
          <Button size="md" onclick={() => (showCreate = true)}><Plus />Create bucket</Button>
        {/if}
      {/snippet}
    </Toolbar>

    {#if !bucketsQuery.isPending && filteredBuckets.length === 0}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title="No matching buckets" />
      </div>
    {:else}
      <Table.Root>
        <Table.Header>
          {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row class="hover:bg-transparent">
              {#each headerGroup.headers as header (header.id)}
                <Table.Head
                  class={header.column.id === 'actions'
                    ? 'w-10'
                    : rightAligned.has(header.column.id)
                      ? 'text-right'
                      : undefined}
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
          {#if bucketsQuery.isPending}
            {#each Array(5) as _, index (index)}
              <Table.Row class="hover:bg-transparent">
                <Table.Cell><Skeleton class="w-40" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-12" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-16" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-16" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-20" /></Table.Cell>
                <Table.Cell><Skeleton class="w-32" /></Table.Cell>
                <Table.Cell></Table.Cell>
                <Table.Cell></Table.Cell>
              </Table.Row>
            {/each}
          {:else}
            {#each table.getRowModel().rows as row (row.id)}
              {@const bucket = row.original}
              {@const saved = savedBytes(bucket)}
              <Table.Row class="cursor-pointer" onclick={() => goto(routes.bucket(bucket.name))}>
                <Table.Cell class="font-medium">
                  <a
                    href={routes.bucket(bucket.name)}
                    class="rounded-control hover:underline"
                    onclick={(e: Event) => e.stopPropagation()}
                  >{bucket.name}</a>
                </Table.Cell>
                <Table.Cell class="num text-right text-muted-foreground">
                  {bucket.objectCount !== null ? bucket.objectCount.toLocaleString() : '—'}
                </Table.Cell>
                <Table.Cell class="num text-right text-muted-foreground">
                  {bucket.sizeBytes !== null ? formatBytes(bucket.sizeBytes) : '—'}
                </Table.Cell>
                <Table.Cell class="num text-right text-muted-foreground">
                  {bucket.storedBytes !== null ? formatBytes(bucket.storedBytes) : '—'}
                </Table.Cell>
                <Table.Cell class="num text-right text-muted-foreground">
                  {#if saved}
                    {formatBytes(saved)} ({Math.round((saved / bucket.sizeBytes!) * 100)}%)
                  {:else}
                    —
                  {/if}
                </Table.Cell>
                <Table.Cell class="num text-muted-foreground">{formatDate(bucket.createdAt)}</Table.Cell>
                <Table.Cell>
                  {#if bucket.publicRead}
                    <Badge variant="warning" label="Public" />
                  {/if}
                </Table.Cell>
                <Table.Cell class="w-10 pr-2 pl-0 text-right">
                  {#if bucket.canManageSettings || bucket.canDelete}
                    <DropdownMenu.Root>
                      <DropdownMenu.Trigger>
                        {#snippet child({ props })}
                          <Button
                            {...props}
                            variant="ghost"
                            size="icon-xs"
                            aria-label="Actions for {bucket.name}"
                            onclick={(e: Event) => e.stopPropagation()}
                          ><MoreHorizontal /></Button>
                        {/snippet}
                      </DropdownMenu.Trigger>
                      <DropdownMenu.Content align="end">
                        {#if bucket.canManageSettings}
                          <DropdownMenu.Item onSelect={() => goto(routes.bucketSettings(bucket.name))}>
                            <Settings />Settings
                          </DropdownMenu.Item>
                        {/if}
                        {#if bucket.canManageSettings && bucket.canDelete}
                          <DropdownMenu.Separator />
                        {/if}
                        {#if bucket.canDelete}
                          <DropdownMenu.Item variant="destructive" onSelect={() => (bucketToDelete = bucket.name)}>
                            <Trash2 />Delete
                          </DropdownMenu.Item>
                        {/if}
                      </DropdownMenu.Content>
                    </DropdownMenu.Root>
                  {/if}
                </Table.Cell>
              </Table.Row>
            {/each}
          {/if}
        </Table.Body>
      </Table.Root>
    {/if}
  {/if}
</div>

<Dialog
  open={showCreate}
  title="Create bucket"
  loading={createBucketMutation.isPending}
  onClose={closeCreate}
>
  <form
    id="create-bucket-form"
    onsubmit={(e) => { e.preventDefault(); createBucket() }}
    class="flex flex-col gap-1.5"
  >
    <Label for="bucket-name">Name</Label>
    <Input
      bind:ref={createBucketInput}
      id="bucket-name"
      type="text"
      bind:value={newBucketName}
      placeholder="bucket-name"
      autocomplete="off"
      disabled={createBucketMutation.isPending}
      aria-invalid={createNameError ? true : undefined}
      aria-describedby={createNameError ? 'bucket-name-error' : undefined}
    />
    {#if createNameError}
      <p id="bucket-name-error" class="text-xs text-danger">{createNameError}</p>
    {/if}
  </form>
  {#snippet footer()}
    <Button type="button" variant="secondary" disabled={createBucketMutation.isPending} onclick={closeCreate}>
      Cancel
    </Button>
    <Button
      type="submit"
      form="create-bucket-form"
      loading={createBucketMutation.isPending}
      disabled={!newBucketName.trim() || createNameError !== null}
    >
      Create
    </Button>
  {/snippet}
</Dialog>

<ConfirmDialog
  open={bucketToDelete !== null}
  title="Delete bucket"
  confirmLabel="Delete"
  confirmVariant="destructive"
  confirmationText={bucketToDelete ?? undefined}
  loading={deleteBucketMutation.isPending}
  onClose={() => (bucketToDelete = null)}
  onConfirm={confirmDeleteBucket}
/>
