<script lang="ts">
  import { goto } from '$app/navigation'
  import { createInfiniteQuery, createMutation, createQuery } from '@tanstack/svelte-query'
  import {
    createColumnHelper,
    createTable,
    FlexRender,
    type SortingState,
  } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader } from '$lib/table/sortable'
  import { serverSortableTableFeatures } from '$lib/table/server-sortable'
  import type { ObjectListSort } from '$lib/api/objects'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import Folder from 'lucide-svelte/icons/folder'
  import FileIcon from 'lucide-svelte/icons/file'
  import ImageIcon from 'lucide-svelte/icons/image'
  import FileText from 'lucide-svelte/icons/file-text'
  import FileCode from 'lucide-svelte/icons/file-code'
  import FileArchive from 'lucide-svelte/icons/file-archive'
  import FileSpreadsheet from 'lucide-svelte/icons/file-spreadsheet'
  import Music from 'lucide-svelte/icons/music'
  import Video from 'lucide-svelte/icons/video'
  import ArrowUp from 'lucide-svelte/icons/arrow-up'
  import Copy from 'lucide-svelte/icons/copy'
  import Download from 'lucide-svelte/icons/download'
  import Upload from 'lucide-svelte/icons/upload'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Share2 from 'lucide-svelte/icons/share-2'
  import Check from 'lucide-svelte/icons/check'
  import Minus from 'lucide-svelte/icons/minus'
  import FolderPlus from 'lucide-svelte/icons/folder-plus'
  import Info from 'lucide-svelte/icons/info'
  import Eye from 'lucide-svelte/icons/eye'
  import MoreHorizontal from 'lucide-svelte/icons/more-horizontal'
  import Search from 'lucide-svelte/icons/search'
  import X from 'lucide-svelte/icons/x'
  import ObjectDetailDrawer from './ObjectDetailDrawer.svelte'
  import FilePreview from './FilePreview.svelte'
  import { formatBytes } from '$lib/format-bytes'
  import { formatDate } from '$lib/format'
  import { isPreviewable } from '$lib/preview'
  import { toast, toastApiError } from '$lib/toast'
  import { bucketKeys, objectKeys, settingsKeys } from '$lib/api/keys'
  import { createFolder as createFolderApi, deleteFolder as deleteFolderApi, deleteFolders as deleteFoldersApi, deleteObject as deleteObjectApi, deleteObjects as deleteObjectsApi, downloadUrl, listObjects, previewFolderDelete, presignObject, uploadObject, type FolderDeletePreview, type S3File } from '$lib/api/objects'
  import { getVersioning } from '$lib/api/settings'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { cn, displayName } from '$lib/utils'
  import { bucketObjectsUrl } from '$lib/navigation'
  import { register } from '$lib/shortcuts'

  interface Props {
    bucket: string
    prefix?: string
  }
  let { bucket, prefix = '' }: Props = $props()
  let searchInput = $state('')
  let searchQuery = $state('')
  let fileInput: HTMLInputElement | undefined = $state()
  let shareKey = $state<string | null>(null)
  let shareExpires = $state(3600)
  let shareUrl = $state('')
  let shareLoading = $state(false)
  type UploadItem = { id: number; name: string; abort?: AbortController; loaded: number; total: number; status: 'queued' | 'uploading' | 'done' | 'error' | 'cancelled' }
  let uploads = $state<UploadItem[]>([])
  let uploadSeq = 0
  let showCreateFolder = $state(false)
  let newFolderName = $state('')
  let detailFile = $state<S3File | null>(null)
  let previewFile = $state<S3File | null>(null)
  let pendingDelete = $state<string | null>(null)
  let pendingDeletePrefix = $state<string | null>(null)
  let showBulkDeleteConfirm = $state(false)
  let folderDeletePreview = $state<FolderDeletePreview | null>(null)
  let folderDeletePreviewLoading = $state(false)
  let selectedKeys = $state<Set<string>>(new Set())
  let createFolderInput = $state<HTMLInputElement | null>(null)
  let sentinelEl = $state<HTMLDivElement | undefined>()
  let dragDepth = $state(0)
  let sorting = $state<SortingState>([])
  let wide = $state(false)
  const isDragging = $derived(dragDepth > 0)

  const listSort = $derived<ObjectListSort>(
    (sorting[0]?.id as ObjectListSort | undefined) ?? 'name',
  )
  const listOrder = $derived(sorting[0]?.desc ? 'desc' : 'asc')

  $effect(() => {
    const query = window.matchMedia('(min-width: 1024px)')
    wide = query.matches
    const update = (event: MediaQueryListEvent) => (wide = event.matches)
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  })

  $effect(() => {
    const unregisterUpload = register('u', () => fileInput?.click(), {
      description: 'Upload files',
      group: 'Objects',
    })
    const unregisterFolder = register('n', () => (showCreateFolder = true), {
      description: 'New folder',
      group: 'Objects',
    })
    return () => {
      unregisterUpload()
      unregisterFolder()
    }
  })

  $effect(() => {
    if (showCreateFolder && createFolderInput) {
      queueMicrotask(() => createFolderInput?.focus())
    }
  })

  $effect(() => {
    prefix
    clearSearch()
    sorting = []
    selectedKeys = new Set()
    detailFile = null
    previewFile = null
    shareKey = null
  })

  $effect(() => {
    const value = searchInput
    const id = setTimeout(() => {
      searchQuery = value.trim()
    }, 300)
    return () => clearTimeout(id)
  })

  const objectsQuery = createInfiniteQuery(() => ({
    queryKey: objectKeys.list(bucket, prefix, searchQuery || undefined, listSort, listOrder),
    queryFn: ({ pageParam }) =>
      listObjects(bucket, prefix, pageParam, searchQuery || undefined, listSort, listOrder),
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
    mutationFn: async (items: { item: UploadItem; file: File }[]) => {
      let uploaded = 0
      for (const { item, file } of items) {
        if (item.status === 'cancelled') continue
        item.status = 'uploading'
        item.abort = new AbortController()
        try {
          await uploadObject(bucket, `${prefix}${file.name}`, file, (loaded, total) => {
            item.loaded = loaded
            item.total = total
          }, item.abort.signal)
        } catch (err) {
          if (err instanceof DOMException && err.name === 'AbortError') {
            item.status = 'cancelled'
            continue
          }
          item.status = 'error'
          throw err
        }
        item.loaded = item.total
        item.status = 'done'
        uploaded++
      }
      return uploaded
    },
    onSuccess: (count) => {
      if (count > 0) toast.success(count === 1 ? 'File uploaded' : `${count} files uploaded`)
      if (fileInput) fileInput.value = ''
      queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  const deleteObjectMutation = createMutation(() => ({
    mutationFn: (key: string) => deleteObjectApi(bucket, key),
    onSuccess: (_data, key) => {
      toast.success(`"${displayName(key)}" deleted`)
      selectedKeys = new Set([...selectedKeys].filter((k) => k !== key))
      if (detailFile?.key === key) detailFile = null
      queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  const deleteSelectionMutation = createMutation(() => ({
    mutationFn: async ({ folders, objects }: { folders: string[]; objects: string[] }) => {
      const [objectResult, folderResult] = await Promise.all([
        objects.length > 0 ? deleteObjectsApi(bucket, objects) : Promise.resolve({ deleted: 0, failed: [] as string[] }),
        folders.length > 0 ? deleteFoldersApi(bucket, folders) : Promise.resolve({ deleted: 0, failed: [] as string[] }),
      ])
      return { objectResult, folderResult }
    },
    onSuccess: ({ objectResult, folderResult }, { folders, objects }) => {
      const foldersOk = folders.length - folderResult.failed.length
      const objectsOk = objects.length - objectResult.failed.length
      const failed = [...objectResult.failed, ...folderResult.failed]

      if (foldersOk > 0 || objectsOk > 0) {
        const parts: string[] = []
        if (foldersOk > 0) parts.push(foldersOk === 1 ? '1 folder' : `${foldersOk} folders`)
        if (objectsOk > 0) parts.push(objectsOk === 1 ? '1 object' : `${objectsOk} objects`)
        let msg = `${parts.join(' and ')} deleted`
        if (foldersOk > 0 && folderResult.deleted > 0) {
          msg += ` (${folderResult.deleted} object${folderResult.deleted === 1 ? '' : 's'} inside)`
        }
        toast.success(msg)
      }
      if (failed.length > 0) {
        toast.error(
          failed.length === 1
            ? `Failed to delete "${displayName(failed[0])}"`
            : `Failed to delete ${failed.length} items`,
        )
      }
      selectedKeys = new Set(failed)
      showBulkDeleteConfirm = false
      queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })
    },
  }))

  const createFolderMutation = createMutation(() => ({
    mutationFn: (name: string) => createFolderApi(bucket, `${prefix}${name}`),
    onSuccess: (_data, name) => {
      toast.success(`Folder "${name}" created`)
      newFolderName = ''
      showCreateFolder = false
      queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })
    },
  }))

  const deleteFolderMutation = createMutation(() => ({
    mutationFn: (folderPrefix: string) => deleteFolderApi(bucket, folderPrefix),
    onSuccess: (result, folderPrefix) => {
      const label = prefixLabel(folderPrefix).replace(/\/$/, '')
      toast.success(
        result.deleted === 0
          ? `Folder "${label}" deleted`
          : `Folder "${label}" deleted (${result.deleted} object${result.deleted === 1 ? '' : 's'})`,
      )
      pendingDeletePrefix = null
      selectedKeys = new Set([...selectedKeys].filter((k) => k !== folderPrefix))
      queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })
    },
  }))

  const files = $derived(objectsQuery.data?.pages.flatMap((page) => page.files) ?? [])
  const fileKeys = $derived(files.map((file) => file.key))
  const prefixes = $derived.by(() => {
    const seen = new Set<string>()
    const result: string[] = []
    for (const page of objectsQuery.data?.pages ?? []) {
      for (const p of page.prefixes) {
        if (p === prefix) continue
        const folder = `${prefix}${p.slice(prefix.length).split('/')[0]}/`
        if (!seen.has(folder)) {
          seen.add(folder)
          result.push(folder)
        }
      }
    }
    return result
  })
  const selectableKeys = $derived([...prefixes, ...fileKeys])
  const selectedCount = $derived(selectedKeys.size)
  const selectedFolderCount = $derived(prefixes.filter((p) => selectedKeys.has(p)).length)
  const selectedObjectCount = $derived(fileKeys.filter((k) => selectedKeys.has(k)).length)
  const selectedFolderPrefixes = $derived(prefixes.filter((p) => selectedKeys.has(p)))
  const selectedObjectStats = $derived.by(() => {
    const selected = files.filter((file) => selectedKeys.has(file.key))
    return {
      count: selected.length,
      sizeBytes: selected.reduce((sum, file) => sum + file.size, 0),
    }
  })
  const allSelected = $derived(selectableKeys.length > 0 && selectableKeys.every((key) => selectedKeys.has(key)))
  const someSelected = $derived(selectableKeys.some((key) => selectedKeys.has(key)))

  type ObjectRow = { id: string }

  const objectColumnHelper = createColumnHelper<typeof serverSortableTableFeatures, ObjectRow>()
  const objectColumns = [
    objectColumnHelper.display({ id: 'select', enableSorting: false, header: '' }),
    objectColumnHelper.accessor('id', { id: 'name', header: sortableHeader('Name') }),
    objectColumnHelper.accessor('id', { id: 'size', header: sortableHeader('Size') }),
    objectColumnHelper.accessor('id', { id: 'modified', header: sortableHeader('Modified') }),
    objectColumnHelper.accessor('id', { id: 'type', header: sortableHeader('Type') }),
    objectColumnHelper.display({ id: 'actions', enableSorting: false, header: '' }),
  ]

  const objectTable = createTable({
    features: serverSortableTableFeatures,
    columns: objectColumns as never,
    data: [],
    state: {
      get sorting() {
        return sorting
      },
    },
    onSortingChange: (updater) => {
      sorting = typeof updater === 'function' ? updater(sorting) : updater
    },
  })

  $effect(() => {
    prefix
    searchQuery
    listSort
    listOrder
    selectedKeys = new Set()
  })

  $effect(() => {
    const folderPrefixes = pendingDeletePrefix
      ? [pendingDeletePrefix]
      : showBulkDeleteConfirm
        ? selectedFolderPrefixes
        : []

    if (folderPrefixes.length === 0) {
      folderDeletePreview = null
      folderDeletePreviewLoading = false
      return
    }

    let cancelled = false
    folderDeletePreview = null
    folderDeletePreviewLoading = true

    previewFolderDelete(bucket, folderPrefixes)
      .then((preview) => {
        if (!cancelled) folderDeletePreview = preview
      })
      .catch((err) => {
        console.error('previewFolderDelete failed:', err)
        if (!cancelled) folderDeletePreview = null
      })
      .finally(() => {
        if (!cancelled) folderDeletePreviewLoading = false
      })

    return () => {
      cancelled = true
    }
  })

  const versioningEnabled = $derived(!!versioningQuery.data?.enabled)

  const expiryOptions = [
    { value: '3600', label: '1 hour' },
    { value: '21600', label: '6 hours' },
    { value: '86400', label: '24 hours' },
    { value: '604800', label: '7 days' },
  ]

  const skeletonRows = [0, 1, 2, 3, 4, 5, 6, 7]

  const upPrefix = $derived(prefix.replace(/[^/]+\/$/, ''))
  const showType = $derived(!(detailFile && wide))

  function columnClass(id: string): string | undefined {
    switch (id) {
      case 'select':
        return 'w-9 p-0'
      case 'name':
        return 'w-full'
      case 'size':
        return 'w-24 text-right'
      case 'modified':
        return 'hidden w-44 sm:table-cell'
      case 'type':
        return 'hidden w-40 md:table-cell'
      case 'actions':
        return 'w-9 p-0 pr-1'
    }
  }

  function clearSearch() {
    searchInput = ''
    searchQuery = ''
  }

  function navigateTo(newPrefix: string) {
    goto(bucketObjectsUrl(bucket, newPrefix))
  }

  function objectLabel(key: string): string {
    if (searchQuery && prefix && key.startsWith(prefix)) {
      return key.slice(prefix.length)
    }
    return displayName(key)
  }

  function prefixLabel(folderPrefix: string): string {
    return folderPrefix.slice(prefix.length).replace(/\/$/, '')
  }

  function fileIconFor(contentType: string, key: string) {
    const type = contentType.split(';')[0].trim().toLowerCase()
    const ext = key.slice(key.lastIndexOf('.') + 1).toLowerCase()
    if (type.startsWith('image/')) return ImageIcon
    if (type.startsWith('audio/')) return Music
    if (type.startsWith('video/')) return Video
    if (['zip', 'tar', 'gz', 'tgz', '7z', 'rar', 'bz2', 'xz'].includes(ext) || /zip|tar|compressed/.test(type)) return FileArchive
    if (['csv', 'xls', 'xlsx', 'tsv'].includes(ext) || /spreadsheet|excel|csv/.test(type)) return FileSpreadsheet
    if (type.includes('json') || type.includes('xml') || type.includes('javascript') || ['js', 'ts', 'json', 'xml', 'yaml', 'yml', 'toml', 'html', 'css', 'rs', 'py', 'sh', 'svelte'].includes(ext)) return FileCode
    if (type.startsWith('text/') || type === 'application/pdf' || ['txt', 'md', 'pdf', 'doc', 'docx', 'log'].includes(ext)) return FileText
    return FileIcon
  }

  function cancelUpload(item: UploadItem) {
    if (item.status === 'queued') item.status = 'cancelled'
    else if (item.status === 'uploading') item.abort?.abort()
  }

  function uploadPercent(item: UploadItem): number {
    if (item.total > 0) return Math.round((item.loaded / item.total) * 100)
    return item.status === 'done' ? 100 : 0
  }

  function dragHasFiles(event: DragEvent): boolean {
    return event.dataTransfer?.types.includes('Files') ?? false
  }

  function onDragEnter(event: DragEvent) {
    if (!dragHasFiles(event) || uploadMutation.isPending) return
    event.preventDefault()
    dragDepth++
  }

  function onDragLeave(event: DragEvent) {
    if (!dragHasFiles(event)) return
    event.preventDefault()
    dragDepth = Math.max(0, dragDepth - 1)
  }

  function onDragOver(event: DragEvent) {
    if (!dragHasFiles(event) || uploadMutation.isPending) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy'
  }

  function onDrop(event: DragEvent) {
    event.preventDefault()
    dragDepth = 0
    if (uploadMutation.isPending) return
    const dropped = event.dataTransfer?.files
    if (dropped?.length) void uploadFiles(dropped)
  }

  async function uploadFiles(files: FileList | File[]) {
    const fileArray = Array.from(files)
    if (fileArray.length === 0) return

    uploads = fileArray.map((file) => ({ id: uploadSeq++, name: file.name, loaded: 0, total: file.size, status: 'queued' }))
    try {
      await uploadMutation.mutateAsync(fileArray.map((file, index) => ({ item: uploads[index], file })))
    } catch (err) {
      console.error('Upload failed:', err)
      toastApiError(err, 'Upload failed')
      if (fileInput) fileInput.value = ''
    }
  }

  async function handleUpload() {
    const inputFiles = fileInput?.files
    if (!inputFiles || inputFiles.length === 0) return
    await uploadFiles(inputFiles)
  }

  async function confirmPendingDelete() {
    if (!pendingDelete) return
    const key = pendingDelete
    try {
      await deleteObjectMutation.mutateAsync(key)
      pendingDelete = null
    } catch (err) {
      console.error('deleteObject failed:', err)
      toastApiError(err, 'Failed to connect to server')
    }
  }

  async function confirmPendingFolderDelete() {
    if (!pendingDeletePrefix) return
    const folderPrefix = pendingDeletePrefix
    try {
      await deleteFolderMutation.mutateAsync(folderPrefix)
    } catch (err) {
      console.error('deleteFolder failed:', err)
      toastApiError(err, 'Failed to delete folder')
    }
  }

  function toggleSelect(key: string) {
    const next = new Set(selectedKeys)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    selectedKeys = next
  }

  function toggleSelectAll() {
    if (allSelected) {
      selectedKeys = new Set()
      return
    }
    selectedKeys = new Set(selectableKeys)
  }

  function requestBulkDelete() {
    if (selectedCount === 0) return
    showBulkDeleteConfirm = true
  }

  function deleteStatsPhrase(count: number, sizeBytes: number): string {
    const objects = count === 1 ? '1 object' : `${count.toLocaleString()} objects`
    return `${objects} (${formatBytes(sizeBytes)})`
  }

  function deleteStatsSuffix(
    folderPreview: FolderDeletePreview | null,
    extraCount: number,
    extraSizeBytes: number,
  ): string {
    if (folderDeletePreviewLoading) return ' Counting objects…'
    if (!folderPreview && extraCount === 0) return ''
    const count = (folderPreview?.count ?? 0) + extraCount
    const sizeBytes = (folderPreview?.sizeBytes ?? 0) + extraSizeBytes
    return ` This will delete ${deleteStatsPhrase(count, sizeBytes)}.`
  }

  function bulkDeleteDescription(): string {
    const folders = selectedFolderCount
    const objects = selectedObjectCount
    const extraCount = objects
    const extraSizeBytes = selectedObjectStats.sizeBytes
    if (folders > 0 && objects > 0) {
      return `This will permanently delete ${folders === 1 ? '1 folder' : `${folders} folders`} (and everything inside ${folders === 1 ? 'it' : 'them'}) and ${objects === 1 ? '1 object' : `${objects} objects`}.${deleteStatsSuffix(folderDeletePreview, extraCount, extraSizeBytes)}`
    }
    if (folders > 0) {
      const base = folders === 1
        ? `This will permanently delete the folder "${prefixLabel(selectedFolderPrefixes[0]).replace(/\/$/, '')}" and everything inside it.`
        : `This will permanently delete ${folders} folders and everything inside them.`
      return `${base}${deleteStatsSuffix(folderDeletePreview, 0, 0)}`
    }
    if (objects === 1) {
      return `This will delete "${displayName([...selectedKeys].find((k) => fileKeys.includes(k))!)}" from this bucket.`
    }
    return `This will permanently delete ${objects} objects from this bucket.`
  }

  function singleFolderDeleteDescription(folderPrefix: string): string {
    const label = prefixLabel(folderPrefix).replace(/\/$/, '')
    const base = `This will permanently delete the folder "${label}" and everything inside it.`
    return `${base}${deleteStatsSuffix(folderDeletePreview, 0, 0)}`
  }

  function bulkDeleteTitle(): string {
    const folders = selectedFolderCount
    const objects = selectedObjectCount
    if (folders > 0 && objects > 0) {
      return `Delete ${selectedCount} items?`
    }
    if (folders > 0) {
      return folders === 1 ? 'Delete folder?' : `Delete ${folders} folders?`
    }
    return objects === 1 ? 'Delete object?' : `Delete ${objects} objects?`
  }

  function bulkDeleteLabel(): string {
    const folders = selectedFolderCount
    const objects = selectedObjectCount
    if (folders > 0 && objects > 0) {
      return `Delete ${selectedCount} items`
    }
    if (folders > 0) {
      return folders === 1 ? 'Delete folder' : `Delete ${folders} folders`
    }
    return objects === 1 ? 'Delete object' : `Delete ${objects} objects`
  }

  async function confirmBulkDelete() {
    if (selectedCount === 0) return
    const selection = [...selectedKeys]
    const folders = selection.filter((k) => prefixes.includes(k))
    const objects = selection.filter((k) => fileKeys.includes(k))
    try {
      await deleteSelectionMutation.mutateAsync({ folders, objects })
    } catch (err) {
      console.error('deleteSelection failed:', err)
      toastApiError(err, 'Failed to delete selection')
    }
  }

  async function generateShareUrl() {
    if (!shareKey) return
    const key = shareKey
    const expires = shareExpires
    shareLoading = true
    try {
      const data = await presignObject(bucket, key, expires)
      if (shareKey === key && shareExpires === expires) shareUrl = data.url
    } catch (err) {
      console.error('shareObject failed:', err)
      toastApiError(err, 'Failed to generate share link')
    } finally {
      shareLoading = false
    }
  }

  function openShare(key: string) {
    shareUrl = ''
    shareExpires = 3600
    shareKey = key
    void generateShareUrl()
  }

  function selectShareExpiry(value: string) {
    shareExpires = Number(value)
    shareUrl = ''
    void generateShareUrl()
  }

  async function copyShareUrl() {
    try {
      await navigator.clipboard.writeText(shareUrl)
      toast.success('Presigned URL copied')
    } catch (err) {
      console.error('copyShareUrl failed:', err)
      toast.error('Failed to copy')
    }
  }

  async function createFolder() {
    const name = newFolderName.trim()
    if (!name) return
    try {
      await createFolderMutation.mutateAsync(name)
    } catch (err) {
      console.error('createFolder failed:', err)
      toastApiError(err, 'Failed to create folder')
    }
  }

  function closeCreateFolder() {
    showCreateFolder = false
    newFolderName = ''
  }

  /** Row clicks open the row unless the click landed on an interactive child. */
  function isRowClick(event: MouseEvent): boolean {
    return !(event.target as HTMLElement).closest('a, button, label, input')
  }
</script>

{#snippet checkbox(checked: boolean, indeterminate: boolean, label: string, onchange: () => void)}
  <label class="flex h-8 w-9 cursor-pointer items-center justify-center">
    <input
      type="checkbox"
      class="peer sr-only"
      {checked}
      {indeterminate}
      aria-label={label}
      {onchange}
    />
    <span
      class="flex size-4 items-center justify-center rounded-control border border-border-strong bg-inset text-accent-foreground transition-colors peer-checked:border-accent peer-checked:bg-accent peer-indeterminate:border-accent peer-indeterminate:bg-accent peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent"
    >
      {#if checked}
        <Check class="size-3" strokeWidth={3} />
      {:else if indeterminate}
        <Minus class="size-3" strokeWidth={3} />
      {/if}
    </span>
  </label>
{/snippet}

<div
  class="flex items-start gap-3"
  role="region"
  aria-label="Object browser drop zone"
  ondragenter={onDragEnter}
  ondragleave={onDragLeave}
  ondragover={onDragOver}
  ondrop={onDrop}
>
  <div class="relative flex min-h-[calc(100dvh-11rem)] min-w-0 flex-1 flex-col gap-3">
    {#if isDragging}
      <div
        class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-surface border border-dashed border-accent bg-background/80 text-sm font-medium"
        aria-hidden="true"
      >
        Drop files to upload
      </div>
    {/if}

    <input
      bind:this={fileInput}
      type="file"
      multiple
      class="hidden"
      onchange={handleUpload}
    />

    <Toolbar>
      {#if selectedCount > 0}
        <span class="num text-sm font-medium">{selectedCount} selected</span>
        <Button
          variant="destructive"
          onclick={requestBulkDelete}
          disabled={deleteSelectionMutation.isPending}
        >
          <Trash2 /> Delete
        </Button>
        <Button variant="ghost" onclick={() => (selectedKeys = new Set())}>Clear</Button>
      {:else}
        {#if prefix}
          <Tooltip.Root>
            <Tooltip.Trigger>
              {#snippet child({ props })}
                <Button
                  {...props}
                  variant="outline"
                  size="icon"
                  href={bucketObjectsUrl(bucket, upPrefix)}
                  aria-label="Parent folder"
                >
                  <ArrowUp />
                </Button>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content>Parent folder</Tooltip.Content>
          </Tooltip.Root>
        {/if}
        <div class="relative min-w-32 max-w-xs flex-1">
          <Search class="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-subtle" />
          <Input
            type="text"
            placeholder="Search in folder"
            class="pl-7 pr-7"
            bind:value={searchInput}
            aria-label="Search objects"
          />
          {#if searchInput}
            <Button
              variant="ghost"
              size="icon-xs"
              class="absolute right-0.5 top-1/2 -translate-y-1/2"
              onclick={clearSearch}
              aria-label="Clear search"
            >
              <X />
            </Button>
          {/if}
        </div>
      {/if}
      {#snippet end()}
        {#if selectedCount === 0 && !objectsQuery.isPending}
          <span class="num hidden text-sm text-muted-foreground sm:inline">
            {(files.length + prefixes.length).toLocaleString()}{objectsQuery.hasNextPage ? '+' : ''} items
          </span>
        {/if}
        <Button variant="secondary" aria-label="New folder" onclick={() => (showCreateFolder = true)}>
          <FolderPlus /> <span class="hidden sm:inline">New folder</span>
        </Button>
        <Button
          aria-label="Upload"
          loading={uploadMutation.isPending}
          onclick={() => fileInput?.click()}
        >
          {#if !uploadMutation.isPending}<Upload />{/if} <span class="hidden sm:inline">Upload</span>
        </Button>
      {/snippet}
    </Toolbar>

    {#if objectsQuery.isError}
      <Callout type="danger">{objectsQuery.error instanceof ApiError ? objectsQuery.error.message : 'Failed to load objects'}</Callout>
    {/if}

    {#if !objectsQuery.isPending && files.length === 0 && prefixes.length === 0 && !objectsQuery.isError}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title={searchQuery ? `No objects matching "${searchQuery}"` : 'No objects'}>
          {#snippet action()}
            {#if !searchQuery}
              <Button onclick={() => fileInput?.click()}><Upload /> Upload</Button>
            {/if}
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <Table.Root>
        <Table.Header>
          {#each objectTable.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row>
              {#each headerGroup.headers as header (header.id)}
                {#if header.column.id !== 'type' || showType}
                  <Table.Head class={columnClass(header.column.id)}>
                    {#if header.column.id === 'select'}
                      {#if selectableKeys.length > 0}
                        {@render checkbox(allSelected, !allSelected && someSelected, allSelected ? 'Deselect all' : 'Select all', toggleSelectAll)}
                      {/if}
                    {:else if !header.isPlaceholder}
                      <FlexRender header={header} />
                    {/if}
                  </Table.Head>
                {/if}
              {/each}
            </Table.Row>
          {/each}
        </Table.Header>
        <Table.Body>
          {#if objectsQuery.isPending}
            {#each skeletonRows as index (index)}
              <Table.Row>
                <Table.Cell class="w-9 p-0"></Table.Cell>
                <Table.Cell><Skeleton class="w-48 max-w-full" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-12" /></Table.Cell>
                <Table.Cell class="hidden sm:table-cell"><Skeleton class="w-32" /></Table.Cell>
                {#if showType}<Table.Cell class="hidden md:table-cell"><Skeleton class="w-24" /></Table.Cell>{/if}
                <Table.Cell class="w-9 p-0"></Table.Cell>
              </Table.Row>
            {/each}
          {/if}
          {#each prefixes as p (p)}
            <Table.Row
              class="cursor-pointer"
              data-state={selectedKeys.has(p) ? 'selected' : undefined}
              onclick={(e) => {
                if (isRowClick(e)) navigateTo(p)
              }}
            >
              <Table.Cell class="w-9 p-0">
                {@render checkbox(selectedKeys.has(p), false, `Select ${prefixLabel(p)}`, () => toggleSelect(p))}
              </Table.Cell>
              <Table.Cell class="w-full max-w-0">
                <a href={bucketObjectsUrl(bucket, p)} class="flex min-w-0 items-center gap-2 rounded-control font-medium hover:underline">
                  <Folder class="size-4 shrink-0 text-muted-foreground" />
                  <span class="truncate">{prefixLabel(p)}/</span>
                </a>
              </Table.Cell>
              <Table.Cell></Table.Cell>
              <Table.Cell class="hidden sm:table-cell"></Table.Cell>
              {#if showType}<Table.Cell class="hidden text-muted-foreground md:table-cell">Folder</Table.Cell>{/if}
              <Table.Cell class="w-9 p-0 pr-1">
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger>
                    {#snippet child({ props })}
                      <Button {...props} variant="ghost" size="icon-xs" aria-label={`Actions for ${prefixLabel(p)}`}>
                        <MoreHorizontal />
                      </Button>
                    {/snippet}
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content>
                    <DropdownMenu.Item variant="destructive" onSelect={() => (pendingDeletePrefix = p)}>
                      <Trash2 /> Delete folder
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </Table.Cell>
            </Table.Row>
          {/each}
          {#each files as file (file.key)}
            {@const RowIcon = fileIconFor(file.contentType, file.key)}
            <Table.Row
              class="cursor-pointer"
              data-state={selectedKeys.has(file.key) || detailFile?.key === file.key ? 'selected' : undefined}
              onclick={(e) => {
                if (isRowClick(e)) detailFile = file
              }}
            >
              <Table.Cell class="w-9 p-0">
                {@render checkbox(selectedKeys.has(file.key), false, `Select ${objectLabel(file.key)}`, () => toggleSelect(file.key))}
              </Table.Cell>
              <Table.Cell class="w-full max-w-0">
                <button
                  type="button"
                  class="flex w-full min-w-0 items-center gap-2 rounded-control text-left font-medium hover:underline"
                  onclick={() => (detailFile = file)}
                >
                  <RowIcon class="size-4 shrink-0 text-muted-foreground" />
                  <span class="truncate">{objectLabel(file.key)}</span>
                </button>
              </Table.Cell>
              <Table.Cell class="num text-right text-muted-foreground">{formatBytes(file.size)}</Table.Cell>
              <Table.Cell class="num hidden text-muted-foreground sm:table-cell">{formatDate(file.lastModified)}</Table.Cell>
              {#if showType}
                <Table.Cell class="hidden max-w-40 truncate text-muted-foreground md:table-cell">{file.contentType || '—'}</Table.Cell>
              {/if}
              <Table.Cell class="w-9 p-0 pr-1">
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger>
                    {#snippet child({ props })}
                      <Button {...props} variant="ghost" size="icon-xs" aria-label={`Actions for ${objectLabel(file.key)}`}>
                        <MoreHorizontal />
                      </Button>
                    {/snippet}
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content>
                    <DropdownMenu.Item onSelect={() => (detailFile = file)}>
                      <Info /> Details
                    </DropdownMenu.Item>
                    {#if isPreviewable(file.contentType, file.key)}
                      <DropdownMenu.Item onSelect={() => (previewFile = file)}>
                        <Eye /> Preview
                      </DropdownMenu.Item>
                    {/if}
                    <DropdownMenu.Item onSelect={() => openShare(file.key)}>
                      <Share2 /> Copy presigned URL
                    </DropdownMenu.Item>
                    <DropdownMenu.Item>
                      {#snippet child({ props })}
                        <a {...props} href={downloadUrl(bucket, file.key)}><Download /> Download</a>
                      {/snippet}
                    </DropdownMenu.Item>
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item variant="destructive" onSelect={() => (pendingDelete = file.key)}>
                      <Trash2 /> Delete
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
      <div bind:this={sentinelEl} class="h-px" aria-hidden="true"></div>
      {#if objectsQuery.isFetchingNextPage}
        <div class="flex justify-center py-1"><Skeleton class="w-24" /></div>
      {/if}
    {/if}
  </div>

  {#if detailFile && wide}
    <ObjectDetailDrawer
      panel
      {bucket}
      file={detailFile}
      {versioningEnabled}
      onClose={() => (detailFile = null)}
      onPreview={(file) => (previewFile = file)}
      onShare={openShare}
      onDelete={(key) => (pendingDelete = key)}
      onVersionDeleted={() => queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })}
    />
  {/if}
</div>

{#if detailFile && !wide}
  <ObjectDetailDrawer
    {bucket}
    file={detailFile}
    {versioningEnabled}
    onClose={() => (detailFile = null)}
    onPreview={(file) => (previewFile = file)}
    onShare={openShare}
    onDelete={(key) => (pendingDelete = key)}
    onVersionDeleted={() => queryClient.invalidateQueries({ queryKey: objectKeys.listScope(bucket, prefix) })}
  />
{/if}

<Dialog
  open={showCreateFolder}
  title="New folder"
  loading={createFolderMutation.isPending}
  onClose={closeCreateFolder}
>
  <form id="create-folder-form" onsubmit={(e) => { e.preventDefault(); createFolder() }} class="flex flex-col gap-1.5">
    <Label for="folder-name">Name</Label>
    <Input
      bind:ref={createFolderInput}
      id="folder-name"
      type="text"
      bind:value={newFolderName}
      placeholder="folder-name"
      disabled={createFolderMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button type="button" variant="secondary" disabled={createFolderMutation.isPending} onclick={closeCreateFolder}>
      Cancel
    </Button>
    <Button type="submit" form="create-folder-form" loading={createFolderMutation.isPending} disabled={!newFolderName.trim()}>
      Create
    </Button>
  {/snippet}
</Dialog>

{#if shareKey}
  <Dialog open title={`Presigned URL: ${displayName(shareKey)}`} onClose={() => (shareKey = null)}>
    <div class="flex flex-col gap-3">
      <div class="flex items-center gap-2">
        <Label>Expires</Label>
        <SegmentedControl
          aria-label="Link expiry"
          options={expiryOptions}
          value={String(shareExpires)}
          onValueChange={selectShareExpiry}
        />
      </div>
      <Input
        readonly
        value={shareLoading ? 'Generating…' : shareUrl}
        class="font-mono text-xs"
        aria-label="Presigned URL"
        onfocus={(e) => (e.currentTarget as HTMLInputElement).select()}
      />
    </div>
    {#snippet footer()}
      <Button type="button" variant="secondary" onclick={() => (shareKey = null)}>Close</Button>
      <Button type="button" disabled={shareLoading || !shareUrl} onclick={copyShareUrl}>
        <Copy /> Copy
      </Button>
    {/snippet}
  </Dialog>
{/if}

{#if uploads.length > 0}
  <div class="fixed bottom-4 right-4 z-40 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2 rounded-surface border border-border-strong bg-raised p-3 shadow-pop" role="region" aria-label="Uploads">
    <div class="flex items-center justify-between">
      <span class="text-sm font-medium">Uploads</span>
      {#if !uploadMutation.isPending}
        <Button variant="ghost" size="icon-xs" onclick={() => (uploads = [])} aria-label="Dismiss uploads">
          <X />
        </Button>
      {/if}
    </div>
    <ul class="flex max-h-64 flex-col gap-2 overflow-y-auto">
      {#each uploads as item (item.id)}
        <li class="flex flex-col gap-1">
          <div class="flex items-center justify-between gap-2 text-xs">
            <span class="min-w-0 flex-1 truncate">{item.name}</span>
            <span class={cn('num shrink-0', item.status === 'error' ? 'text-danger' : 'text-muted-foreground')}>
              {#if item.status === 'uploading'}
                {uploadPercent(item)}%
              {:else if item.status === 'queued'}
                Queued
              {:else if item.status === 'done'}
                Done
              {:else if item.status === 'error'}
                Failed
              {:else}
                Cancelled
              {/if}
            </span>
            {#if item.status === 'queued' || item.status === 'uploading'}
              <Button variant="ghost" size="icon-xs" onclick={() => cancelUpload(item)} aria-label={`Cancel upload of ${item.name}`}>
                <X />
              </Button>
            {/if}
          </div>
          <ProgressBar value={uploadPercent(item)} dangerAt={item.status === 'error' ? 0 : undefined} />
        </li>
      {/each}
    </ul>
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
    description={`This will delete "${displayName(pendingDelete)}" from this bucket.`}
    confirmLabel="Delete object"
    confirmVariant="destructive"
    loading={deleteObjectMutation.isPending}
    onClose={() => (pendingDelete = null)}
    onConfirm={confirmPendingDelete}
  />
{/if}

{#if showBulkDeleteConfirm}
  <ConfirmDialog
    open
    title={bulkDeleteTitle()}
    description={bulkDeleteDescription()}
    confirmLabel={bulkDeleteLabel()}
    confirmVariant="destructive"
    loading={deleteSelectionMutation.isPending || (selectedFolderCount > 0 && folderDeletePreviewLoading)}
    onClose={() => (showBulkDeleteConfirm = false)}
    onConfirm={confirmBulkDelete}
  />
{/if}

{#if pendingDeletePrefix}
  <ConfirmDialog
    open
    title="Delete folder?"
    description={singleFolderDeleteDescription(pendingDeletePrefix)}
    confirmLabel="Delete folder"
    confirmVariant="destructive"
    loading={deleteFolderMutation.isPending || folderDeletePreviewLoading}
    onClose={() => (pendingDeletePrefix = null)}
    onConfirm={confirmPendingFolderDelete}
  />
{/if}
