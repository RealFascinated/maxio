<script lang="ts">
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { createQuery } from '@tanstack/svelte-query'
  import { bucketKeys } from '$lib/api/keys'
  import { listBuckets } from '$lib/api/buckets'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { Tabs } from '$lib/components/ui/tabs'
  import { toast } from '$lib/toast'
  import { formatBytes } from '$lib/format-bytes'
  import { bucketObjectsUrl, routes } from '$lib/navigation'

  let { children } = $props()

  const bucket = $derived(decodeURIComponent(page.params.bucket!))
  const isSettings = $derived(page.url.pathname.endsWith('/settings'))

  const bucketsQuery = createQuery(() => ({
    queryKey: bucketKeys.list(),
    queryFn: listBuckets,
  }))

  const bucketStats = $derived(
    bucketsQuery.data?.buckets.find((entry) => entry.name === bucket),
  )

  $effect(() => {
    if (bucketsQuery.isSuccess && !bucketsQuery.isFetching && !bucketStats) {
      toast.error(`Bucket "${bucket}" not found`)
      goto(routes.home(), { replaceState: true })
    }
  })

  const tabs = $derived([
    {
      value: 'objects',
      label: 'Objects',
      href: bucketObjectsUrl(bucket),
      count: bucketStats?.objectCount ?? undefined,
    },
    { value: 'settings', label: 'Settings', href: routes.bucketSettings(bucket) },
  ])
</script>

<div class="flex flex-col gap-3">
  <PageHeader title={bucket}>
    {#snippet actions()}
      {#if bucketStats?.sizeBytes != null}
        <span class="num text-sm text-muted-foreground">
          {formatBytes(bucketStats.sizeBytes)}
          {#if bucketStats.storedBytes != null && bucketStats.storedBytes < bucketStats.sizeBytes}
            · {formatBytes(bucketStats.storedBytes)} on disk
          {/if}
        </span>
      {/if}
    {/snippet}
  </PageHeader>
  <Tabs items={tabs} value={isSettings ? 'settings' : 'objects'} aria-label="Bucket sections" />
  <div>
    {@render children()}
  </div>
</div>
