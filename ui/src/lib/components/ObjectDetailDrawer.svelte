<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import { SlideOver } from '$lib/components/ui/slide-over'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Tabs } from '$lib/components/ui/tabs'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import Copy from 'lucide-svelte/icons/copy'
  import Download from 'lucide-svelte/icons/download'
  import Eye from 'lucide-svelte/icons/eye'
  import Share2 from 'lucide-svelte/icons/share-2'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import X from 'lucide-svelte/icons/x'
  import VersionHistory from './VersionHistory.svelte'
  import FilePreview from './FilePreview.svelte'
  import { isPreviewable, previewKind } from '$lib/preview'
  import { objectKeys } from '$lib/api/keys'
  import { downloadUrl, getObjectDetail, type S3File } from '$lib/api/objects'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { formatBytes } from '$lib/format-bytes'
  import { formatDate } from '$lib/format'
  import { toast } from '$lib/toast'
  import { displayName, truncateId } from '$lib/utils'

  interface Props {
    bucket: string
    file: S3File
    versioningEnabled?: boolean
    /** Render as an inline side panel instead of a slide-over. */
    panel?: boolean
    onClose: () => void
    onPreview: (file: S3File) => void
    onShare: (key: string) => void
    onDelete: (key: string) => void
    onVersionDeleted?: () => void
  }
  let {
    bucket,
    file,
    versioningEnabled = false,
    panel = false,
    onClose,
    onPreview,
    onShare,
    onDelete,
    onVersionDeleted,
  }: Props = $props()

  let tab = $state('details')

  const detailQuery = createQuery(() => ({
    queryKey: objectKeys.detail(bucket, file.key),
    queryFn: () => getObjectDetail(bucket, file.key),
  }))

  const detail = $derived(detailQuery.data)
  const tags = $derived(Object.entries(detail?.tags ?? {}))
  const name = $derived(displayName(file.key))
  const tabs = $derived([
    { value: 'details', label: 'Details' },
    ...(versioningEnabled ? [{ value: 'versions', label: 'Versions' }] : []),
  ])

  $effect(() => {
    file.key
    tab = 'details'
  })

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast.success(`${label} copied`)
    } catch (err) {
      console.error('copyText failed:', err)
      toast.error('Failed to copy')
    }
  }

  function formatEtag(etag: string): string {
    return etag.replace(/^"|"$/g, '')
  }

  function handleVersionDeleted() {
    queryClient.invalidateQueries({ queryKey: objectKeys.detail(bucket, file.key) })
    onVersionDeleted?.()
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape' || event.defaultPrevented) return
    event.preventDefault()
    onClose()
  }
</script>

{#snippet row(label: string, value: string, options: { mono?: boolean; copy?: string; display?: string } = {})}
  <div class="grid min-h-8 grid-cols-[84px_minmax(0,1fr)] items-center gap-2 border-b border-border py-1 last:border-b-0">
    <dt class="text-muted-foreground">{label}</dt>
    <dd class="flex min-w-0 items-center gap-1">
      <span class="min-w-0 flex-1 break-all {options.mono ? 'font-mono text-xs' : 'num'}">{options.display ?? value}</span>
      {#if options.copy}
        <Tooltip.Root>
          <Tooltip.Trigger>
            {#snippet child({ props })}
              <Button
                {...props}
                variant="ghost"
                size="icon-xs"
                aria-label={`Copy ${label.toLowerCase()}`}
                onclick={() => copyText(options.copy!, label)}
              >
                <Copy />
              </Button>
            {/snippet}
          </Tooltip.Trigger>
          <Tooltip.Content>Copy {label.toLowerCase()}</Tooltip.Content>
        </Tooltip.Root>
      {/if}
    </dd>
  </div>
{/snippet}

{#snippet body()}
  <div class="flex flex-col gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <Button variant="secondary" size="sm" href={downloadUrl(bucket, file.key)}>
        <Download /> Download
      </Button>
      <Button variant="secondary" size="sm" onclick={() => onShare(file.key)}>
        <Share2 /> Share
      </Button>
      {#if isPreviewable(file.contentType, file.key)}
        <Button variant="secondary" size="sm" onclick={() => onPreview(file)}>
          <Eye /> Preview
        </Button>
      {/if}
      <Button variant="ghost" size="sm" class="text-danger hover:text-danger" onclick={() => onDelete(file.key)}>
        <Trash2 /> Delete
      </Button>
    </div>

    {#if detailQuery.isError}
      <Callout type="danger">
        {detailQuery.error instanceof ApiError ? detailQuery.error.message : 'Failed to load object details'}
      </Callout>
    {/if}

    {#if previewKind(file.contentType, file.key) === 'image'}
      <div class="rounded-control border border-border bg-inset p-2">
        <FilePreview inline {bucket} objectKey={file.key} contentType={file.contentType} size={file.size} />
      </div>
    {/if}

    {#if tabs.length > 1}
      <Tabs items={tabs} bind:value={tab} aria-label="Object sections" />
    {/if}

    {#if tab === 'versions'}
      <VersionHistory {bucket} objectKey={file.key} onVersionDeleted={handleVersionDeleted} />
    {:else}
      <dl class="text-sm">
        {@render row('Key', detail?.key ?? file.key, { mono: true, copy: detail?.key ?? file.key })}
        {@render row('Size', formatBytes(detail?.size ?? file.size))}
        {@render row('Modified', formatDate(detail?.lastModified ?? file.lastModified))}
        {@render row('Type', (detail?.contentType ?? file.contentType) || '—')}
        {@render row('ETag', formatEtag(detail?.etag ?? file.etag), {
          mono: true,
          copy: formatEtag(detail?.etag ?? file.etag),
        })}
        {#if detail?.versionId}
          {@render row('Version ID', detail.versionId, {
            mono: true,
            copy: detail.versionId,
            display: truncateId(detail.versionId),
          })}
        {/if}
      </dl>

      <div class="flex flex-col gap-1">
        <h3 class="text-sm font-medium">Tags</h3>
        {#if detailQuery.isPending}
          <Skeleton class="h-6 w-full" />
        {:else if tags.length === 0}
          <p class="text-sm text-muted-foreground">No tags</p>
        {:else}
          <dl class="text-sm">
            {#each tags as [key, value] (key)}
              {@render row(key, value)}
            {/each}
          </dl>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

{#if panel}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <aside
    aria-label="Object details"
    class="sticky top-0 flex max-h-[calc(100dvh-5.5rem)] w-96 shrink-0 flex-col rounded-surface border border-border bg-panel"
    onkeydown={handleKeydown}
  >
    <div class="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border pl-3 pr-2">
      <h2 class="min-w-0 truncate text-sm font-semibold">{name}</h2>
      <Button variant="ghost" size="icon-xs" aria-label="Close details" onclick={onClose}><X /></Button>
    </div>
    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      {@render body()}
    </div>
  </aside>
{:else}
  <SlideOver open title={name} {onClose}>
    {@render body()}
  </SlideOver>
{/if}
