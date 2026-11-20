<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { Button } from '$lib/components/ui/button'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import { jobKeys } from '$lib/api/keys'
  import { cancelJob, listJobs, startJob } from '$lib/api/jobs'
  import { getCompressionEstimate } from '$lib/api/settings'
  import { formatBytes } from '$lib/format-bytes'
  import { queryClient } from '$lib/query/client'
  import { toast, toastApiError } from '$lib/toast'

  let { bucket, compressionEnabled }: { bucket: string; compressionEnabled: boolean } = $props()

  const queryParams = () => ({ kind: 'compressBucket' as const, scope: bucket, limit: 1 })

  const jobQuery = createQuery(() => ({
    queryKey: jobKeys.list(queryParams()),
    queryFn: () => listJobs(queryParams()),
    refetchInterval: (query) => (query.state.data?.jobs[0]?.status === 'running' ? 1500 : false),
  }))

  const job = $derived(jobQuery.data?.jobs[0] ?? null)
  const compressionJob = $derived(job?.kind === 'compressBucket' ? job : null)
  const running = $derived(compressionJob?.status === 'running')
  const percent = $derived(
    compressionJob && compressionJob.total && compressionJob.total > 0
      ? (compressionJob.processed / compressionJob.total) * 100
      : 0,
  )

  // Progress only shows while a job runs; the outcome is announced once when it ends.
  let lastSeen: { id: string; status: string } | null = null
  $effect(() => {
    const current = compressionJob
    if (!current) return
    if (lastSeen?.id === current.id && lastSeen.status === 'running' && current.status !== 'running') {
      if (current.status === 'completed') {
        const { compressed, skipped, bytesBefore, bytesAfter } = current.detail
        toast.success(
          `Compressed ${compressed.toLocaleString()} objects (${skipped.toLocaleString()} skipped), saved ${formatBytes(Math.max(0, bytesBefore - bytesAfter))}`,
        )
      } else if (current.status === 'failed') {
        toast.error(current.error ?? 'Compression failed')
      }
    }
    lastSeen = { id: current.id, status: current.status }
  })

  const estimateMutation = createMutation(() => ({
    mutationFn: () => getCompressionEstimate(bucket),
    onError: (err) => {
      console.error('estimateCompression failed:', err)
      toastApiError(err, 'Failed to estimate compression')
    },
  }))
  const estimate = $derived(estimateMutation.data)

  const startMutation = createMutation(() => ({
    mutationFn: () => startJob({ kind: 'compressBucket', scope: bucket }),
    onSuccess: () => {
      toast.success('Compression started')
      queryClient.invalidateQueries({ queryKey: jobKeys.all })
    },
    onError: (err) => {
      console.error('startJob failed:', err)
      toastApiError(err, 'Failed to start compression')
    },
  }))

  const cancelMutation = createMutation(() => ({
    mutationFn: (id: string) => cancelJob(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: jobKeys.all }),
    onError: (err) => {
      console.error('cancelJob failed:', err)
      toastApiError(err, 'Failed to cancel compression')
    },
  }))
</script>

<div class="flex flex-col gap-2 rounded-surface border border-border bg-panel p-3">
  <div class="flex items-center justify-between gap-4">
    <span>Compress existing objects</span>
    <div class="flex items-center gap-2">
      <Button
        variant="secondary"
        size="sm"
        loading={estimateMutation.isPending}
        onclick={() => estimateMutation.mutate()}
      >Estimate savings</Button>
      {#if running && compressionJob}
        <Button
          variant="outline"
          size="sm"
          loading={cancelMutation.isPending}
          onclick={() => cancelMutation.mutate(compressionJob.id)}
        >
          Cancel
        </Button>
      {:else}
        <Button
          size="sm"
          disabled={!compressionEnabled}
          loading={startMutation.isPending}
          onclick={() => startMutation.mutate()}
        >Compress</Button>
      {/if}
    </div>
  </div>

  {#if estimate}
    <div class="num text-muted-foreground">
      {#if estimate.sampledObjects === 0}
        Nothing to compress
      {:else}
        ~{formatBytes(estimate.estimatedSavedBytes)} saved of {formatBytes(estimate.candidateBytes)}
        ({estimate.ratio.toFixed(2)}× on {estimate.sampledObjects.toLocaleString()} sampled
        {estimate.sampledObjects === 1 ? 'object' : 'objects'})
      {/if}
    </div>
  {/if}

  {#if running && compressionJob}
    <ProgressBar value={compressionJob.status === 'completed' ? 100 : percent} />
    <div class="num flex flex-wrap gap-x-4 text-muted-foreground">
      <span class="capitalize">
        {compressionJob.status}{running ? ` · ${compressionJob.detail.phase}` : ''}
      </span>
      <span>
        {compressionJob.processed.toLocaleString()} /
        {compressionJob.total == null ? '?' : compressionJob.total.toLocaleString()}
      </span>
      <span>{compressionJob.detail.compressed.toLocaleString()} compressed</span>
      <span>{compressionJob.detail.skipped.toLocaleString()} skipped</span>
      {#if compressionJob.detail.failed > 0}
        <span class="text-danger">{compressionJob.detail.failed.toLocaleString()} failed</span>
      {/if}
      <span>
        {formatBytes(Math.max(0, compressionJob.detail.bytesBefore - compressionJob.detail.bytesAfter))} saved
      </span>
    </div>
    {#if compressionJob.error}
      <div class="text-danger">{compressionJob.error}</div>
    {/if}
  {/if}
</div>
