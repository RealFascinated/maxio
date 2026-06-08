<script lang="ts">
  import { onMount } from 'svelte'
  import { createInfiniteQuery, createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import Folder from 'lucide-svelte/icons/folder'
  import FileIcon from 'lucide-svelte/icons/file'
  import Download from 'lucide-svelte/icons/download'
  import Upload from 'lucide-svelte/icons/upload'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Share2 from 'lucide-svelte/icons/share-2'
  import Check from 'lucide-svelte/icons/check'
  import FolderPlus from 'lucide-svelte/icons/folder-plus'
  import History from 'lucide-svelte/icons/history'
  import Eye from 'lucide-svelte/icons/eye'
  import Search from 'lucide-svelte/icons/search'
  import X from 'lucide-svelte/icons/x'
  import VersionHistory from './VersionHistory.svelte'
  import FilePreview from './FilePreview.svelte'
  import { isPreviewable } from '$lib/preview'
  import { toast } from '$lib/toast'
  import { objectKeys, settingsKeys } from '$lib/api/keys'
  import { createFolder as createFolderApi, deleteObject as deleteObjectApi, downloadUrl, listObjects, presignObject, uploadObject, type S3File } from '$lib/api/objects'
  import { getVersioning } from '$lib/api/settings'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { displayName } from '$lib/utils'

  interface Props {
    bucket: string
    onBack: () => void
    onPrefixChange?: (prefix: string, breadcrumbs: { label: string; prefix: string }[]) => void
  }
  let { bucket, onBack, onPrefixChange }: Props = $props()

  let prefix = $state('')
  let searchInput = $state('')
  let searchQuery = $state('')
  let fileInput: HTMLInputElement | undefined = $state()
  let copiedKey = $state<string | null>(null)
  let shareMenuKey = $state<string | null>(null)
  let showCreateFolder = $state(false)
  let newFolderName = $state('')
  let shareMenuPos = $state({ top: 0, left: 0 })
  let versionKey = $state<string | null>(null)
  let previewFile = $state<S3File | null>(null)
  let pendingDelete = $state<string | null>(null)
  let createFolderInput = $state<HTMLInputElement | null>(null)
  let sentinelEl = $state<HTMLDivElement | undefined>()

  $effect(() => {
    if (showCreateFolder && createFolderInput) {
      queueMicrotask(() => createFolderInput?.focus())
    }
  })

  $effect(() => {
    const value = searchInput
    const id = setTimeout(() => {
      searchQuery = value.trim()
    }, 300)
    return () => clearTimeout(id)
  })

  const objectsQuery = createInfiniteQuery(() => ({
    queryKey: objectKeys.list(bucket, prefix, searchQuery || undefined),
    queryFn: ({ pageParam }) =>
      listObjects(bucket, prefix, pageParam, searchQuery || undefined),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) => lastPage.nextContinuationToken ?? undefined,
  }))

  function maybeFetchNextPage() {
    if (objectsQuery.hasNextPage && !objectsQuery.isFetchingNextPage && !objectsQuery.isPending) {
      objectsQuery.fetchNextPage()
    }
  }

  $effect(() => {
    const el = sentinelEl
    if (!el) return

    const observer = new IntersectionObserver((entries) => {
      if (entries[0]?.isIntersecting) {
        maybeFetchNextPage()
      }
    })
    observer.observe(el)
    return () => observer.disconnect()
  })

  $effect(() => {
    const el = sentinelEl
    objectsQuery.data?.pages.length
    if (!el) return

    const rect = el.getBoundingClientRect()
    const inView = rect.top < window.innerHeight && rect.bottom > 0
    if (inView) {
      maybeFetchNextPage()
    }
  })

  const versioningQuery = createQuery(() => ({
    queryKey: settingsKeys.versioning(bucket),
    queryFn: () => getVersioning(bucket),
  }))

  const uploadMutation = createMutation(() => ({
    mutationFn: async (files: FileList) => {
      for (const file of Array.from(files)) {
        await uploadObject(bucket, `${prefix}${file.name}`, file)
      }
      return files.length
    },
    onSuccess: (count) => {
      toast.success(count === 1 ? 'File uploaded' : `${count} files uploaded`)
      if (fileInput) fileInput.value = ''
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const deleteObjectMutation = createMutation(() => ({
    mutationFn: (key: string) => deleteObjectApi(bucket, key),
    onSuccess: (_data, key) => {
      toast.success(`"${displayName(key)}" deleted`)
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const createFolderMutation = createMutation(() => ({
    mutationFn: (name: string) => createFolderApi(bucket, `${prefix}${name}`),
    onSuccess: (_data, name) => {
      toast.success(`Folder "${name}" created`)
      newFolderName = ''
      showCreateFolder = false
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const files = $derived(objectsQuery.data?.pages.flatMap((page) => page.files) ?? [])
  const prefixes = $derived.by(() => {
    const seen = new Set<string>()
    const result: string[] = []
    for (const page of objectsQuery.data?.pages ?? []) {
      for (const p of page.prefixes) {
        if (p !== prefix && !seen.has(p)) {
          seen.add(p)
          result.push(p)
        }
      }
    }
    return result
  })
  const versioningEnabled = $derived(!!versioningQuery.data?.enabled)

  const expiryOptions = [
    { label: '1 hour', seconds: 3600 },
    { label: '6 hours', seconds: 21600 },
    { label: '24 hours', seconds: 86400 },
    { label: '7 days', seconds: 604800 },
  ]


  function notifyPrefix() {
    onPrefixChange?.(prefix, breadcrumbs)
  }

  function clearSearch() {
    searchInput = ''
    searchQuery = ''
  }

  export function navigateTo(newPrefix: string) {
    prefix = newPrefix
    clearSearch()
    notifyPrefix()
  }

  export function goUp() {
    if (!prefix) {
      onBack()
      return
    }
    const trimmed = prefix.slice(0, -1)
    const lastSlash = trimmed.lastIndexOf('/')
    prefix = lastSlash >= 0 ? trimmed.slice(0, lastSlash + 1) : ''
    clearSearch()
    notifyPrefix()
  }

  function objectLabel(key: string): string {
    if (searchQuery && prefix && key.startsWith(prefix)) {
      return key.slice(prefix.length)
    }
    return displayName(key)
  }

  function prefixLabel(folderPrefix: string): string {
    if (searchQuery && prefix && folderPrefix.startsWith(prefix)) {
      return folderPrefix.slice(prefix.length)
    }
    return displayName(folderPrefix)
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
  }

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString()
    } catch {
      return iso
    }
  }

  let breadcrumbs = $derived.by(() => {
    const parts = prefix.split('/').filter(Boolean)
    const crumbs: { label: string; prefix: string }[] = [
      { label: bucket, prefix: '' },
    ]
    let acc = ''
    for (const part of parts) {
      acc += part + '/'
      crumbs.push({ label: part, prefix: acc })
    }
    return crumbs
  })

  async function handleUpload() {
    const inputFiles = fileInput?.files
    if (!inputFiles || inputFiles.length === 0) return
    const toastId = toast.loading(inputFiles.length === 1 ? `Uploading ${inputFiles[0].name}…` : `Uploading ${inputFiles.length} files…`)
    try {
      await uploadMutation.mutateAsync(inputFiles)
      toast.dismiss(toastId)
    } catch (err) {
      console.error('Upload failed:', err)
      toast.error(err instanceof Error ? err.message : 'Upload failed', { id: toastId })
      if (fileInput) fileInput.value = ''
    }
  }

  async function deleteObject(key: string, e: Event) {
    e.stopPropagation()
    pendingDelete = key
  }

  async function confirmPendingDelete() {
    if (!pendingDelete) return
    const key = pendingDelete
    try {
      await deleteObjectMutation.mutateAsync(key)
      pendingDelete = null
    } catch (err) {
      console.error('deleteObject failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to connect to server')
    }
  }

  function toggleShareMenu(key: string, e: MouseEvent) {
    e.stopPropagation()
    if (shareMenuKey === key) {
      shareMenuKey = null
      return
    }
    const btn = e.currentTarget as HTMLElement
    const rect = btn.getBoundingClientRect()
    shareMenuPos = { top: rect.top, left: rect.right }
    shareMenuKey = key
  }

  async function shareObject(key: string, expires: number) {
    shareMenuKey = null
    try {
      const data = await presignObject(bucket, key, expires)
      await navigator.clipboard.writeText(data.url)
      copiedKey = key
      setTimeout(() => { copiedKey = null }, 2000)
      toast.success('Presigned URL copied to clipboard')
    } catch (err) {
      console.error('shareObject failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to generate share link')
    }
  }

  async function createFolder() {
    const name = newFolderName.trim()
    if (!name) return
    try {
      await createFolderMutation.mutateAsync(name)
    } catch (err) {
      console.error('createFolder failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to create folder')
    }
  }

  function handleClickOutside() {
    if (shareMenuKey) shareMenuKey = null
  }

  onMount(() => {
    document.addEventListener('click', handleClickOutside)
    return () => document.removeEventListener('click', handleClickOutside)
  })
</script>

<div class="flex flex-col gap-4">
  {#if objectsQuery.isError}
    <Callout type="danger">{objectsQuery.error instanceof ApiError ? objectsQuery.error.message : 'Failed to load objects'}</Callout>
  {/if}

  <div class="flex flex-wrap items-center gap-2">
    <input
      bind:this={fileInput}
      type="file"
      multiple
      class="hidden"
      onchange={handleUpload}
    />
    <Button variant="highlighted" class="h-8" onclick={() => fileInput?.click()} disabled={uploadMutation.isPending}>
      <Upload class="size-4 mr-1" /> {uploadMutation.isPending ? 'Uploading...' : 'Upload'}
    </Button>
    <Button variant="outline" class="h-8" onclick={() => (showCreateFolder = true)}>
      <FolderPlus class="size-4 mr-1" /> New Folder
    </Button>
    <div class="relative min-w-[12rem] flex-1 max-w-sm">
      <Search class="pointer-events-none absolute left-2 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        type="search"
        placeholder="Search in this folder…"
        class="h-8 pl-8 pr-8"
        bind:value={searchInput}
        aria-label="Search objects"
      />
      {#if searchInput}
        <button
          type="button"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground"
          onclick={clearSearch}
          aria-label="Clear search"
        >
          <X class="size-4" />
        </button>
      {/if}
    </div>
  </div>

  {#if objectsQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading...</p>
  {:else if files.length === 0 && prefixes.length === 0 && !objectsQuery.isError}
    <Callout type="info">
      <span class="inline-flex items-center gap-2">
        {#if searchQuery}
          <Search class="size-4 opacity-70" />
          No objects matching &ldquo;{searchQuery}&rdquo; in this location.
        {:else}
          <Folder class="size-4 opacity-70" />
          This location is empty — upload a file or create a folder to get started.
        {/if}
      </span>
    </Callout>
  {:else}
    <Table.Root>
      <Table.Header>
        <Table.Row>
          <Table.Head>Name</Table.Head>
          <Table.Head class="w-48">Type</Table.Head>
          <Table.Head class="w-28 text-right">Size</Table.Head>
          <Table.Head class="w-48">Modified</Table.Head>
          <Table.Head class="w-24"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each prefixes as p}
          <Table.Row class="cursor-pointer" onclick={() => navigateTo(p)}>
            <Table.Cell>
              <span class="flex items-center gap-2">
                <Folder class="size-4 shrink-0 text-muted-foreground" />
                <span class="font-medium">{prefixLabel(p).replace(/\/$/, '')}/</span>
              </span>
            </Table.Cell>
            <Table.Cell class="text-muted-foreground">&mdash;</Table.Cell>
            <Table.Cell class="text-right text-muted-foreground">&mdash;</Table.Cell>
            <Table.Cell class="text-muted-foreground">&mdash;</Table.Cell>
            <Table.Cell></Table.Cell>
          </Table.Row>
        {/each}
        {#each files as file}
          <Table.Row>
            <Table.Cell>
              <span class="flex items-center gap-2">
                <FileIcon class="size-4 shrink-0 text-muted-foreground" />
                <span class="font-medium">{objectLabel(file.key)}</span>
              </span>
            </Table.Cell>
            <Table.Cell class="truncate text-muted-foreground">{file.contentType || '—'}</Table.Cell>
            <Table.Cell class="text-right text-muted-foreground">{formatSize(file.size)}</Table.Cell>
            <Table.Cell class="text-muted-foreground">{formatDate(file.lastModified)}</Table.Cell>
            <Table.Cell class="w-24 text-right">
              <span class="flex items-center justify-end gap-4">
                {#if isPreviewable(file.contentType)}
                  <button
                    class="text-muted-foreground hover:text-foreground transition-colors"
                    onclick={(e) => { e.stopPropagation(); previewFile = file }}
                    title="Preview"
                  >
                    <Eye class="size-4" />
                  </button>
                {/if}
                {#if versioningEnabled}
                  <button
                    class="text-muted-foreground hover:text-foreground transition-colors"
                    onclick={(e) => { e.stopPropagation(); versionKey = versionKey === file.key ? null : file.key }}
                    title="Version history"
                  >
                    <History class="size-4" />
                  </button>
                {/if}
                <button
                  class="text-muted-foreground hover:text-foreground transition-colors"
                  onclick={(e) => toggleShareMenu(file.key, e)}
                  title="Copy presigned URL"
                >
                  {#if copiedKey === file.key}
                    <Check class="size-4 text-green-500" />
                  {:else}
                    <Share2 class="size-4" />
                  {/if}
                </button>
                <a href={downloadUrl(bucket, file.key)} class="text-muted-foreground hover:text-foreground" onclick={(e) => e.stopPropagation()} title="Download">
                  <Download class="size-4" />
                </a>
                <button
                  class="text-muted-foreground hover:text-destructive transition-colors"
                  onclick={(e) => deleteObject(file.key, e)}
                  title="Delete"
                >
                  <Trash2 class="size-4" />
                </button>
              </span>
            </Table.Cell>
          </Table.Row>
          {#if versionKey === file.key}
            <Table.Row>
              <Table.Cell colspan={5} class="p-0">
                <div class="p-2">
                  <VersionHistory
                    {bucket}
                    objectKey={file.key}
                    onClose={() => (versionKey = null)}
                    onVersionDeleted={() => queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })}
                  />
                </div>
              </Table.Cell>
            </Table.Row>
          {/if}
        {/each}
      </Table.Body>
    </Table.Root>
    <div bind:this={sentinelEl} class="h-1" aria-hidden="true"></div>
    {#if objectsQuery.isFetchingNextPage}
      <p class="text-sm text-muted-foreground text-center py-2">Loading more...</p>
    {/if}
  {/if}
</div>


<Dialog
  open={showCreateFolder}
  title="Create folder"
  description="Create an empty folder marker in the current location."
  loading={createFolderMutation.isPending}
  onClose={() => { showCreateFolder = false; newFolderName = '' }}
>
  <form id="create-folder-form" onsubmit={(e) => { e.preventDefault(); createFolder() }} class="flex flex-col gap-1.5">
    <label for="folder-name" class="text-sm font-medium text-black dark:text-white">Folder name</label>
    <Input
      bind:ref={createFolderInput}
      id="folder-name"
      type="text"
      bind:value={newFolderName}
      placeholder="folder-name"
      class="bg-white dark:bg-base"
      disabled={createFolderMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button type="button" variant="default" disabled={createFolderMutation.isPending} onclick={() => { showCreateFolder = false; newFolderName = '' }}>
      Cancel
    </Button>
    <Button type="submit" form="create-folder-form" variant="highlighted" disabled={createFolderMutation.isPending || !newFolderName.trim()}>
      {createFolderMutation.isPending ? 'Creating…' : 'Create folder'}
    </Button>
  {/snippet}
</Dialog>

{#if shareMenuKey}
  <div
    class="fixed z-50 min-w-[8rem] rounded-sm border bg-popover p-1 shadow-md"
    style="top: {shareMenuPos.top}px; left: {shareMenuPos.left}px; transform: translate(-100%, -100%);"
    role="menu"
  >
    {#each expiryOptions as opt}
      <button
        class="w-full rounded-sm px-2 py-1.5 text-left text-sm text-popover-foreground hover:bg-accent hover:text-accent-foreground"
        onclick={() => shareObject(shareMenuKey!, opt.seconds)}
      >
        {opt.label}
      </button>
    {/each}
  </div>
{/if}

{#if previewFile}
  <FilePreview
    {bucket}
    objectKey={previewFile.key}
    contentType={previewFile.contentType}
    size={previewFile.size}
    onClose={() => (previewFile = null)}
  />
{/if}

{#if pendingDelete}
  <ConfirmDialog
    open
    title="Delete object?"
    description={`This will delete \"${displayName(pendingDelete)}\" from this bucket.`}
    confirmLabel="Delete object"
    confirmVariant="destructive"
    loading={deleteObjectMutation.isPending}
    onClose={() => (pendingDelete = null)}
    onConfirm={confirmPendingDelete}
  />
{/if}
