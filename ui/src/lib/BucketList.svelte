<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Badge } from '$lib/components/ui/badge'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import Database from 'lucide-svelte/icons/database'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Settings from 'lucide-svelte/icons/settings'
  import Search from 'lucide-svelte/icons/search'
  import X from 'lucide-svelte/icons/x'
  import { toast } from '$lib/toast'
  import { bucketKeys } from '$lib/api/keys'
  import { createBucket as createBucketApi, deleteBucket as deleteBucketApi, listBuckets } from '$lib/api/buckets'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'

  interface Props {
    onSelect: (bucket: string) => void
    onSettings: (bucket: string) => void
    canCreateBucket?: boolean
  }
  let { onSelect, onSettings, canCreateBucket = true }: Props = $props()

  let showCreate = $state(false)
  let newBucketName = $state('')
  let bucketToDelete = $state<string | null>(null)
  let createBucketInput = $state<HTMLInputElement | null>(null)
  let searchInput = $state('')

  $effect(() => {
    if (showCreate && createBucketInput) {
      queueMicrotask(() => createBucketInput?.focus())
    }
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

  function formatSize(bytes: number): string {
    if (bytes === 0) return '0 B'
    const units = ['B', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(1024))
    const value = bytes / Math.pow(1024, i)
    return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`
  }

  const createBucketMutation = createMutation(() => ({
    mutationFn: createBucketApi,
    onSuccess: (_data, name) => {
      toast.success(`Bucket "${name}" created`)
      newBucketName = ''
      showCreate = false
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  const deleteBucketMutation = createMutation(() => ({
    mutationFn: deleteBucketApi,
    onSuccess: (_data, name) => {
      toast.success(`Bucket "${name}" deleted`)
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))


  async function createBucket() {
    const name = newBucketName.trim()
    if (!name) return
    try {
      await createBucketMutation.mutateAsync(name)
    } catch (err) {
      console.error('createBucket failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to connect to server')
    }
  }

  async function deleteBucket(name: string, e: Event) {
    e.stopPropagation()
    bucketToDelete = name
  }

  async function confirmDeleteBucket() {
    if (!bucketToDelete) return
    const name = bucketToDelete
    try {
      await deleteBucketMutation.mutateAsync(name)
      bucketToDelete = null
    } catch (err) {
      console.error('deleteBucket failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to connect to server')
    }
  }

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString()
    } catch {
      return iso
    }
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between gap-4">
    <div class="flex items-center gap-2">
      <Database class="size-5 text-coollabs dark:text-warning" />
      <h2 class="text-lg font-semibold">Buckets</h2>
    </div>
    {#if canCreateBucket}
      <Button variant="brand" onclick={() => (showCreate = true)}>
        <Plus class="size-4" />
        Create bucket
      </Button>
    {/if}
  </div>

  {#if bucketsQuery.isError}
    <Callout type="danger">{bucketsQuery.error instanceof ApiError ? bucketsQuery.error.message : 'Failed to load buckets'}</Callout>
  {:else if bucketsQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading...</p>
  {:else if allBuckets.length === 0}
    <Callout type="info">
      <div class="flex flex-col gap-3">
        <span class="inline-flex items-center gap-2">
          <Database class="size-4 opacity-70" />
          No buckets yet — create your first bucket to get started.
        </span>
        {#if canCreateBucket}
          <Button variant="brand" class="w-fit" onclick={() => (showCreate = true)}>
            <Plus class="size-4" />
            Create bucket
          </Button>
        {/if}
      </div>
    </Callout>
  {:else}
    <div class="relative max-w-sm">
      <Search class="pointer-events-none absolute left-2 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        type="search"
        placeholder="Filter buckets…"
        class="h-8 pl-8 pr-8"
        bind:value={searchInput}
        aria-label="Filter buckets by name"
      />
      {#if searchInput}
        <button
          type="button"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground"
          onclick={() => (searchInput = '')}
          aria-label="Clear filter"
        >
          <X class="size-4" />
        </button>
      {/if}
    </div>

    {#if filteredBuckets.length === 0}
      <Callout type="info">
        <span class="inline-flex items-center gap-2">
          <Search class="size-4 opacity-70" />
          No buckets matching &ldquo;{searchInput.trim()}&rdquo;.
        </span>
      </Callout>
    {:else}
    <Table.Root>
      <Table.Header>
        <Table.Row>
          <Table.Head>Name</Table.Head>
          <Table.Head>Objects</Table.Head>
          <Table.Head>Size</Table.Head>
          <Table.Head>Versioning</Table.Head>
          <Table.Head>Created</Table.Head>
          <Table.Head class="w-20"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each filteredBuckets as bucket}
          <Table.Row class="cursor-pointer" onclick={() => onSelect(bucket.name)}>
            <Table.Cell class="font-medium">{bucket.name}</Table.Cell>
            <Table.Cell class="text-muted-foreground tabular-nums">
              {#if bucket.objectCount !== null}
                {bucket.objectCount.toLocaleString()}
              {:else}
                <span class="opacity-40">—</span>
              {/if}
            </Table.Cell>
            <Table.Cell class="text-muted-foreground tabular-nums">
              {#if bucket.sizeBytes !== null}
                {formatSize(bucket.sizeBytes)}
              {:else}
                <span class="opacity-40">—</span>
              {/if}
            </Table.Cell>
            <Table.Cell>
              {#if bucket.versioning}
                <Badge variant="success" label="Enabled" />
              {:else}
                <span class="text-xs text-muted-foreground">Disabled</span>
              {/if}
            </Table.Cell>
            <Table.Cell class="text-muted-foreground">{formatDate(bucket.createdAt)}</Table.Cell>
            <Table.Cell class="w-20">
              {#if bucket.canManageSettings || bucket.canDelete}
                <div class="flex items-center gap-4">
                  {#if bucket.canManageSettings}
                    <button
                      class="text-muted-foreground hover:text-foreground transition-colors"
                      onclick={(e: Event) => { e.stopPropagation(); onSettings(bucket.name) }}
                      title="Bucket settings"
                    >
                      <Settings class="size-4" />
                    </button>
                  {/if}
                  {#if bucket.canDelete}
                    <button
                      class="text-muted-foreground hover:text-destructive transition-colors"
                      onclick={(e: Event) => deleteBucket(bucket.name, e)}
                      title="Delete bucket"
                    >
                      <Trash2 class="size-4" />
                    </button>
                  {/if}
                </div>
              {/if}
            </Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
    {/if}
  {/if}
</div>


<Dialog
  open={showCreate}
  title="Create bucket"
  description="Choose a unique bucket name for your objects."
  loading={createBucketMutation.isPending}
  onClose={() => { showCreate = false; newBucketName = '' }}
>
  <form id="create-bucket-form" onsubmit={(e) => { e.preventDefault(); createBucket() }} class="flex flex-col gap-1.5">
    <label for="bucket-name" class="text-sm font-medium text-black dark:text-white">Bucket name</label>
    <Input
      bind:ref={createBucketInput}
      id="bucket-name"
      type="text"
      bind:value={newBucketName}
      placeholder="bucket-name"
      class="bg-white dark:bg-base"
      disabled={createBucketMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button type="button" variant="default" disabled={createBucketMutation.isPending} onclick={() => { showCreate = false; newBucketName = '' }}>
      Cancel
    </Button>
    <Button type="submit" form="create-bucket-form" variant="highlighted" disabled={createBucketMutation.isPending || !newBucketName.trim()}>
      {createBucketMutation.isPending ? 'Creating…' : 'Create bucket'}
    </Button>
  {/snippet}
</Dialog>

<ConfirmDialog
  open={bucketToDelete !== null}
  title="Delete bucket?"
  description={`This will delete bucket \"${bucketToDelete ?? ''}\". The bucket must be empty before it can be removed.`}
  confirmLabel="Delete bucket"
  confirmVariant="destructive"
  confirmationText={bucketToDelete ?? undefined}
  confirmationLabel="Bucket name"
  loading={deleteBucketMutation.isPending}
  onClose={() => (bucketToDelete = null)}
  onConfirm={confirmDeleteBucket}
/>
