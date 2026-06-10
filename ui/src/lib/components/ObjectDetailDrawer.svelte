<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import { SlideOver } from '$lib/components/ui/slide-over'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import Copy from 'lucide-svelte/icons/copy'
  import Download from 'lucide-svelte/icons/download'
  import Loader2 from 'lucide-svelte/icons/loader-2'
  import VersionHistory from './VersionHistory.svelte'
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
    onClose: () => void
    onVersionDeleted?: () => void
  }
  let { bucket, file, versioningEnabled = false, onClose, onVersionDeleted }: Props = $props()

  const detailQuery = createQuery(() => ({
    queryKey: objectKeys.detail(bucket, file.key),
    queryFn: () => getObjectDetail(bucket, file.key),
  }))

  const detail = $derived(detailQuery.data)
  const tags = $derived(Object.entries(detail?.tags ?? {}))

  const sectionTitleClass = 'text-xs font-medium uppercase tracking-wide text-neutral-500'
  const fieldLabelClass = 'mb-1.5 text-xs text-neutral-500'
  const valueBoxClass =
    'flex min-h-8 items-center gap-1.5 rounded-sm border border-neutral-200 bg-white px-2.5 py-1.5 dark:border-coolgray-200 dark:bg-coolgray-100'
  const valueTextClass = 'min-w-0 flex-1 font-mono text-xs text-neutral-800 dark:text-neutral-200'
  const copyButtonClass =
    'shrink-0 text-muted-foreground transition-colors hover:text-foreground'

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
    return etag.startsWith('"') ? etag : `"${etag}"`
  }

  function handleVersionDeleted() {
    queryClient.invalidateQueries({ queryKey: objectKeys.detail(bucket, file.key) })
    onVersionDeleted?.()
  }
</script>

<SlideOver
  open
  title={displayName(file.key)}
  description={file.contentType || 'application/octet-stream'}
  {onClose}
>
  {#if detailQuery.isError}
    <Callout type="danger">
      {detailQuery.error instanceof ApiError ? detailQuery.error.message : 'Failed to load object details'}
    </Callout>
  {/if}

  <div class="flex flex-col gap-6">
    <section>
      <h3 class={sectionTitleClass}>Object</h3>
      <dl class="mt-3 flex flex-col gap-3">
        <div>
          <dt class={fieldLabelClass}>Key</dt>
          <dd class={valueBoxClass}>
            <code class="{valueTextClass} break-all">{detail?.key ?? file.key}</code>
            <button
              type="button"
              class={copyButtonClass}
              aria-label="Copy object key"
              title="Copy key"
              onclick={() => copyText(detail?.key ?? file.key, 'Key')}
            >
              <Copy class="size-3.5" />
            </button>
          </dd>
        </div>
        <div class="grid gap-3 sm:grid-cols-2">
          <div>
            <dt class={fieldLabelClass}>Size</dt>
            <dd class="text-sm tabular-nums">{formatBytes(detail?.size ?? file.size)}</dd>
          </div>
          <div>
            <dt class={fieldLabelClass}>Modified</dt>
            <dd class="text-sm">{formatDate(detail?.lastModified ?? file.lastModified)}</dd>
          </div>
          <div class="sm:col-span-2">
            <dt class={fieldLabelClass}>Content type</dt>
            <dd class="text-sm break-all">{(detail?.contentType ?? file.contentType) || '—'}</dd>
          </div>
          <div class="sm:col-span-2">
            <dt class={fieldLabelClass}>ETag</dt>
            <dd class={valueBoxClass}>
              <code class="{valueTextClass} truncate">{formatEtag(detail?.etag ?? file.etag)}</code>
              <button
                type="button"
                class={copyButtonClass}
                aria-label="Copy ETag"
                title="Copy ETag"
                onclick={() => copyText(formatEtag(detail?.etag ?? file.etag), 'ETag')}
              >
                <Copy class="size-3.5" />
              </button>
            </dd>
          </div>
          {#if detail?.versionId}
            <div class="sm:col-span-2">
              <dt class={fieldLabelClass}>Version ID</dt>
              <dd class={valueBoxClass}>
                <code class={valueTextClass} title={detail.versionId}>{truncateId(detail.versionId)}</code>
                <button
                  type="button"
                  class={copyButtonClass}
                  aria-label="Copy version ID"
                  title="Copy version ID"
                  onclick={() => copyText(detail.versionId!, 'Version ID')}
                >
                  <Copy class="size-3.5" />
                </button>
              </dd>
            </div>
          {/if}
        </div>
      </dl>
    </section>

    <section>
      <h3 class={sectionTitleClass}>Tags</h3>
      {#if detailQuery.isPending}
        <div class="mt-3 flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 class="size-4 animate-spin" />
          Loading tags…
        </div>
      {:else if tags.length === 0}
        <p class="mt-3 text-sm text-muted-foreground">No tags on this object.</p>
      {:else}
        <dl class="mt-3 flex flex-col gap-2">
          {#each tags as [key, value]}
            <div class="{valueBoxClass} justify-between gap-3">
              <dt class="shrink-0 font-mono text-xs text-neutral-500">{key}</dt>
              <dd class="min-w-0 break-all text-right text-xs font-medium">{value}</dd>
            </div>
          {/each}
        </dl>
      {/if}
    </section>

    {#if versioningEnabled}
      <section>
        <h3 class={sectionTitleClass}>Versions</h3>
        <div class="mt-3">
          <VersionHistory
            {bucket}
            objectKey={file.key}
            embedded
            onVersionDeleted={handleVersionDeleted}
          />
        </div>
      </section>
    {/if}
  </div>

  {#snippet footer()}
    <Button variant="default" onclick={onClose}>Close</Button>
    <Button href={downloadUrl(bucket, file.key)} variant="highlighted">
      <Download class="size-4 mr-1" />
      Download
    </Button>
  {/snippet}
</SlideOver>
