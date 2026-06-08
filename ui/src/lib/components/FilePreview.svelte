<script lang="ts">
  import { onMount } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { Dialog } from '$lib/components/ui/dialog'
  import Download from 'lucide-svelte/icons/download'
  import { downloadUrl as objectDownloadUrl } from '$lib/api/objects'
  import { displayName } from '$lib/utils'
  import { previewKind, BINARY_PREVIEW_CAP, TEXT_PREVIEW_CAP, type PreviewKind } from '$lib/preview'

  interface Props {
    bucket: string
    objectKey: string
    contentType: string
    size: number
    onClose: () => void
  }
  let { bucket, objectKey, contentType, size, onClose }: Props = $props()

  const kind: PreviewKind = $derived(previewKind(contentType))
  const name = $derived(displayName(objectKey))
  const downloadUrl = $derived(objectDownloadUrl(bucket, objectKey))
  const tooLarge = $derived(size > BINARY_PREVIEW_CAP)

  let loading = $state(true)
  let error = $state<string | null>(null)
  let blobUrl = $state<string | null>(null)
  let textContent = $state('')
  let truncated = $state(false)

  async function load() {
    if (kind === 'unsupported' || tooLarge) {
      loading = false
      return
    }
    try {
      const res = await fetch(downloadUrl, { credentials: 'same-origin' })
      if (!res.ok) throw new Error(`Request failed (${res.status})`)
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
      console.error('FilePreview load failed:', err)
      error = err instanceof Error ? err.message : 'Failed to load preview'
    } finally {
      loading = false
    }
  }

  onMount(() => {
    load()
    document.body.classList.add('preview-open')
    return () => {
      document.body.classList.remove('preview-open')
      if (blobUrl) URL.revokeObjectURL(blobUrl)
    }
  })
</script>

<Dialog open size="lg" title={name} description={contentType || 'unknown type'} {onClose}>
  {#if loading}
    <p class="text-sm text-neutral-600 dark:text-neutral-400">Loading preview…</p>
  {:else if error}
    <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
  {:else if kind === 'unsupported'}
    <p class="text-sm text-neutral-600 dark:text-neutral-400">No preview available for this file type.</p>
  {:else if tooLarge}
    <p class="text-sm text-neutral-600 dark:text-neutral-400">File is too large to preview. Download it to view.</p>
  {:else if kind === 'image' && blobUrl}
    <img src={blobUrl} alt={name} class="mx-auto max-h-[70vh] max-w-full object-contain" />
  {:else if kind === 'pdf' && blobUrl}
    <iframe src={blobUrl} title={name} class="h-[70vh] w-full border-0"></iframe>
  {:else if kind === 'text'}
    {#if truncated}
      <p class="mb-2 text-xs text-amber-600 dark:text-amber-400">Preview truncated to the first {Math.round(TEXT_PREVIEW_CAP / 1024)} KB.</p>
    {/if}
    <pre class="select-text overflow-auto whitespace-pre-wrap break-words rounded-sm bg-neutral-50 p-3 font-mono text-xs text-neutral-800 dark:bg-coolgray-200 dark:text-neutral-200">{textContent}</pre>
  {/if}
  {#snippet footer()}
    <Button href={downloadUrl} variant="default">
      <Download class="size-4 mr-1" /> Download
    </Button>
  {/snippet}
</Dialog>
