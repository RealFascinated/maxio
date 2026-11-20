<script lang="ts">
  import { onMount } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Dialog } from '$lib/components/ui/dialog'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import Download from 'lucide-svelte/icons/download'
  import { downloadUrl as objectDownloadUrl } from '$lib/api/objects'
  import { notifyUnauthorized, isSessionExpiredError } from '$lib/api/session'
  import { displayName } from '$lib/utils'
  import { previewKind, BINARY_PREVIEW_CAP, TEXT_PREVIEW_CAP, tryPrettyJson, type PreviewKind } from '$lib/preview'

  interface Props {
    bucket: string
    objectKey: string
    contentType: string
    size: number
    inline?: boolean
    onClose?: () => void
  }
  let { bucket, objectKey, contentType, size, inline = false, onClose }: Props = $props()

  const kind: PreviewKind = $derived(previewKind(contentType, objectKey))
  const name = $derived(displayName(objectKey))
  const downloadUrl = $derived(objectDownloadUrl(bucket, objectKey))
  const tooLarge = $derived(size > BINARY_PREVIEW_CAP)

  let loading = $state(true)
  let error = $state<string | null>(null)
  let blobUrl = $state<string | null>(null)
  let textContent = $state('')
  let truncated = $state(false)
  let showOriginal = $state(false)

  const prettyJson = $derived(tryPrettyJson(textContent))
  const displayText = $derived(prettyJson && !showOriginal ? prettyJson : textContent)

  async function load() {
    if (kind === 'unsupported' || tooLarge) {
      loading = false
      return
    }
    try {
      const res = await fetch(downloadUrl, { credentials: 'same-origin' })
      if (!res.ok) {
        if (res.status === 401 && notifyUnauthorized(downloadUrl)) return
        throw new Error(`Request failed: ${res.status}`)
      }
      if (kind === 'text') {
        let text = await res.text()
        if (text.length > TEXT_PREVIEW_CAP) {
          text = text.slice(0, TEXT_PREVIEW_CAP)
          truncated = true
        }
        textContent = text
      } else {
        blobUrl = URL.createObjectURL(await res.blob())
      }
    } catch (err) {
      if (isSessionExpiredError(err)) return
      console.error('FilePreview load failed:', err)
      error = err instanceof Error ? err.message : 'Failed to load preview'
    } finally {
      loading = false
    }
  }

  onMount(() => {
    load()
    if (!inline) document.body.classList.add('preview-open')
    return () => {
      if (!inline) document.body.classList.remove('preview-open')
      if (blobUrl) URL.revokeObjectURL(blobUrl)
    }
  })
</script>

{#snippet body()}
  {#if loading}
    <Skeleton class="h-48 w-full" />
  {:else if error}
    <p class="text-sm text-danger">{error}</p>
  {:else if kind === 'unsupported'}
    <p class="text-sm text-muted-foreground">No preview for this file type</p>
  {:else if tooLarge}
    <p class="text-sm text-muted-foreground">Too large to preview</p>
  {:else if kind === 'image' && blobUrl}
    <img src={blobUrl} alt={name} class="mx-auto {inline ? 'max-h-64' : 'max-h-[70vh]'} max-w-full object-contain" />
  {:else if kind === 'pdf' && blobUrl}
    <iframe src={blobUrl} title={name} class="h-[70vh] w-full rounded-control border border-border"></iframe>
  {:else if kind === 'text'}
    {#if truncated || prettyJson}
      <div class="mb-2 flex items-center justify-between gap-2">
        <span class="num text-xs text-warning">
          {#if truncated}Truncated to first {Math.round(TEXT_PREVIEW_CAP / 1024)} KB{/if}
        </span>
        {#if prettyJson}
          <SegmentedControl
            size="xs"
            aria-label="JSON view"
            options={[
              { value: 'formatted', label: 'Formatted' },
              { value: 'original', label: 'Original' },
            ]}
            value={showOriginal ? 'original' : 'formatted'}
            onValueChange={(next) => (showOriginal = next === 'original')}
          />
        {/if}
      </div>
    {/if}
    <pre class="select-text max-h-[70vh] overflow-auto whitespace-pre-wrap break-words rounded-control border border-border bg-inset p-3 font-mono text-xs">{displayText}</pre>
  {/if}
{/snippet}

{#if inline}
  {@render body()}
{:else}
  <Dialog open size="lg" title={name} {onClose}>
    {@render body()}
    {#snippet footer()}
      <Button href={downloadUrl} variant="secondary">
        <Download /> Download
      </Button>
    {/snippet}
  </Dialog>
{/if}
