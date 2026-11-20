<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { ApiError } from '$lib/api/http'
  import { jobKeys } from '$lib/api/keys'
  import {
    JOB_KIND_LABELS,
    cancelJob,
    listJobs,
    startJob,
    type Job,
    type JobKind,
    type JobStatus,
  } from '$lib/api/jobs'
  import { Badge, type BadgeVariant } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import { Select } from '$lib/components/ui/select'
  import * as Table from '$lib/components/ui/table'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import TriangleAlert from 'lucide-svelte/icons/triangle-alert'
  import { formatBytes } from '$lib/format-bytes'
  import { formatDate, formatDuration } from '$lib/format'
  import { queryClient } from '$lib/query/client'
  import { toast, toastApiError } from '$lib/toast'

  let kindFilter = $state('all')
  let statusFilter = $state('all')
  let expandedId = $state<string | null>(null)

  const params = $derived({
    ...(kindFilter !== 'all' ? { kind: kindFilter as JobKind } : {}),
    ...(statusFilter !== 'all' ? { status: statusFilter as JobStatus } : {}),
  })

  const jobsQuery = createQuery(() => ({
    queryKey: jobKeys.list(params),
    queryFn: () => listJobs(params),
    refetchInterval: (query) =>
      query.state.data?.jobs.some((job) => job.status === 'running') ? 2000 : 15_000,
  }))

  const jobs = $derived(jobsQuery.data?.jobs ?? [])

  const kindOptions = [
    { value: 'all', label: 'All kinds' },
    ...(Object.keys(JOB_KIND_LABELS) as JobKind[]).map((kind) => ({
      value: kind,
      label: JOB_KIND_LABELS[kind],
    })),
  ]

  const statusOptions = [
    { value: 'all', label: 'All statuses' },
    { value: 'running', label: 'Running' },
    { value: 'completed', label: 'Completed' },
    { value: 'cancelled', label: 'Cancelled' },
    { value: 'failed', label: 'Failed' },
  ]

  const statusVariant: Record<JobStatus, BadgeVariant> = {
    running: 'info',
    completed: 'success',
    cancelled: 'neutral',
    failed: 'danger',
  }

  const runActions: { kind: JobKind; label: string }[] = [
    { kind: 'housekeeping', label: 'Run housekeeping' },
    { kind: 'lifecycle', label: 'Run lifecycle' },
    { kind: 'orphanMetaRepair', label: 'Run orphan repair' },
  ]

  function summary(job: Job): string {
    switch (job.kind) {
      case 'compressBucket':
        return `saved ${formatBytes(Math.max(0, job.detail.bytesBefore - job.detail.bytesAfter))}`
      case 'housekeeping':
        return `${job.detail.uploadsRemoved} uploads, ${job.detail.tempFilesRemoved} temp files removed`
      case 'lifecycle':
        return `${job.detail.expired} expired`
      case 'orphanMetaRepair':
        return `${job.detail.removed} removed`
    }
  }

  function duration(job: Job): string {
    const start = new Date(job.startedAt).getTime()
    const end = job.finishedAt ? new Date(job.finishedAt).getTime() : Date.now()
    if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return '—'
    return formatDuration((end - start) / 1000)
  }

  function percent(job: Job): number {
    if (job.total == null || job.total <= 0) return 0
    return (job.processed / job.total) * 100
  }

  const startMutation = createMutation(() => ({
    mutationFn: (kind: JobKind) => startJob({ kind }),
    onSuccess: (data) => {
      toast.success(`${JOB_KIND_LABELS[data.job.kind]} started`)
      queryClient.invalidateQueries({ queryKey: jobKeys.all })
    },
    onError: (err) => {
      console.error('startJob failed:', err)
      if (err instanceof ApiError && err.status === 409) {
        toast.error('Job already running')
        return
      }
      toastApiError(err, 'Failed to start job')
    },
  }))

  const cancelMutation = createMutation(() => ({
    mutationFn: (id: string) => cancelJob(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: jobKeys.all }),
    onError: (err) => {
      console.error('cancelJob failed:', err)
      toastApiError(err, 'Failed to cancel job')
    },
  }))
</script>

<div class="flex flex-col gap-3">
  <PageHeader title="Jobs" />

  <Toolbar>
    <Select
      bind:value={kindFilter}
      size="xs"
      aria-label="Filter jobs by kind"
      options={kindOptions}
      class="w-44"
    />
    <Select
      bind:value={statusFilter}
      size="xs"
      aria-label="Filter jobs by status"
      options={statusOptions}
      class="w-40"
    />
    {#snippet end()}
      {#each runActions as action (action.kind)}
        <Button
          variant="secondary"
          size="sm"
          loading={startMutation.isPending && startMutation.variables === action.kind}
          disabled={startMutation.isPending}
          onclick={() => startMutation.mutate(action.kind)}
        >
          {action.label}
        </Button>
      {/each}
    {/snippet}
  </Toolbar>

  {#if jobsQuery.isError}
    <Callout type="danger">
      {jobsQuery.error instanceof Error ? jobsQuery.error.message : 'Failed to load jobs'}
    </Callout>
  {:else if !jobsQuery.isPending && jobs.length === 0}
    <div class="rounded-surface border border-border bg-panel">
      <EmptyState title="No jobs yet" />
    </div>
  {:else}
    <Table.Root>
      <Table.Header>
        <Table.Row class="hover:bg-transparent">
          <Table.Head>Kind</Table.Head>
          <Table.Head>Scope</Table.Head>
          <Table.Head>Status</Table.Head>
          <Table.Head>Progress</Table.Head>
          <Table.Head>Summary</Table.Head>
          <Table.Head>Started</Table.Head>
          <Table.Head>Duration</Table.Head>
          <Table.Head class="w-10"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each jobs as job (job.id)}
          <Table.Row>
            <Table.Cell>{JOB_KIND_LABELS[job.kind]}</Table.Cell>
            <Table.Cell class="max-w-48 truncate text-muted-foreground">{job.scope ?? '—'}</Table.Cell>
            <Table.Cell>
              <Badge variant={statusVariant[job.status]} label={job.status} class="capitalize" />
            </Table.Cell>
            <Table.Cell>
              {#if job.status === 'running' && job.total != null && job.total > 0}
                <div class="flex items-center gap-2">
                  <ProgressBar value={percent(job)} class="w-20" />
                  <span class="num text-xs text-muted-foreground">
                    {job.processed.toLocaleString()} / {job.total.toLocaleString()}
                  </span>
                </div>
              {:else}
                <span class="num">{job.processed.toLocaleString()}</span>
              {/if}
            </Table.Cell>
            <Table.Cell class="text-muted-foreground">{summary(job)}</Table.Cell>
            <Table.Cell class="num text-muted-foreground">{formatDate(job.startedAt)}</Table.Cell>
            <Table.Cell class="num text-muted-foreground">{duration(job)}</Table.Cell>
            <Table.Cell class="text-right">
              {#if job.status === 'running'}
                <Button
                  variant="outline"
                  size="xs"
                  loading={cancelMutation.isPending && cancelMutation.variables === job.id}
                  onclick={() => cancelMutation.mutate(job.id)}
                >
                  Cancel
                </Button>
              {:else if job.error}
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="ghost"
                        size="icon-xs"
                        class="hover:text-danger"
                        aria-label="Show error"
                        onclick={() => (expandedId = expandedId === job.id ? null : job.id)}
                      >
                        <TriangleAlert />
                      </Button>
                    {/snippet}
                  </Tooltip.Trigger>
                  <Tooltip.Content>Show error</Tooltip.Content>
                </Tooltip.Root>
              {/if}
            </Table.Cell>
          </Table.Row>
          {#if expandedId === job.id && job.error}
            <Table.Row class="hover:bg-transparent">
              <Table.Cell colspan={8} class="whitespace-normal text-xs text-danger">{job.error}</Table.Cell>
            </Table.Row>
          {/if}
        {/each}
      </Table.Body>
    </Table.Root>
  {/if}
</div>
