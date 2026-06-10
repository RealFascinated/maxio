<script lang="ts">
  import { goto } from '$app/navigation'
  import { page } from '$app/state'
  import { createQuery } from '@tanstack/svelte-query'
  import ArrowLeft from 'lucide-svelte/icons/arrow-left'
  import ChevronRight from 'lucide-svelte/icons/chevron-right'
  import { bucketKeys } from '$lib/api/keys'
  import { listBuckets } from '$lib/api/buckets'
  import { formatBytes } from '$lib/format-bytes'
  import { bucketObjectsUrl, pathToPrefix, routes } from '$lib/navigation'

  let { children } = $props()

  const bucket = $derived(decodeURIComponent(page.params.bucket!))
  const isSettings = $derived(page.url.pathname.endsWith('/settings'))
  const objectPrefix = $derived(
    isSettings ? '' : pathToPrefix(page.params.prefix),
  )

  const breadcrumbs = $derived.by(() => {
    const parts = objectPrefix.split('/').filter(Boolean)
    const crumbs: { label: string; prefix: string }[] = [{ label: bucket, prefix: '' }]
    let acc = ''
    for (const part of parts) {
      acc += `${part}/`
      crumbs.push({ label: part, prefix: acc })
    }
    return crumbs
  })

  const bucketsQuery = createQuery(() => ({
    queryKey: bucketKeys.list(),
    queryFn: listBuckets,
  }))

  const bucketStats = $derived(
    bucketsQuery.data?.buckets.find((entry) => entry.name === bucket),
  )

  function goHome() {
    goto(routes.home())
  }

  function goBack() {
    if (isSettings) {
      goHome()
      return
    }
    if (!objectPrefix) {
      goHome()
      return
    }
    const trimmed = objectPrefix.slice(0, -1)
    const lastSlash = trimmed.lastIndexOf('/')
    const parentPrefix = lastSlash >= 0 ? trimmed.slice(0, lastSlash + 1) : ''
    goto(bucketObjectsUrl(bucket, parentPrefix))
  }

</script>

<div
  class="flex min-h-14 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b px-6 py-2 -mx-6 -mt-6 mb-6"
  style="border-color: var(--cool-sidebar-border);"
>
  <button
    type="button"
    onclick={goBack}
    class="shrink-0 rounded-sm p-1 text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
    aria-label={isSettings || !objectPrefix ? 'Back to buckets' : 'Go up one folder'}
  >
    <ArrowLeft class="size-4" />
  </button>
  <nav aria-label="Breadcrumb" class="min-w-0 overflow-x-auto">
    <ol class="flex flex-wrap items-center gap-1.5 text-sm font-medium">
      <li class="inline-flex items-center gap-1.5">
        <a
          href={routes.home()}
          class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
        >Buckets</a>
      </li>
      <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
        <ChevronRight class="size-3 shrink-0" />
      </li>
      {#if isSettings}
        <li class="inline-flex items-center gap-1.5">
          <a
            href={bucketObjectsUrl(bucket)}
            class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
          >{bucket}</a>
        </li>
        <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
          <ChevronRight class="size-3 shrink-0" />
        </li>
        <li class="inline-flex items-center gap-1.5">
          <span class="shrink-0 text-black dark:text-white" aria-current="page">Settings</span>
        </li>
      {:else if breadcrumbs.length > 1}
        {#each breadcrumbs as crumb, i}
          {#if i < breadcrumbs.length - 1}
            <li class="inline-flex items-center gap-1.5">
              <a
                href={bucketObjectsUrl(bucket, crumb.prefix)}
                class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
              >{crumb.label}</a>
            </li>
            <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
              <ChevronRight class="size-3 shrink-0" />
            </li>
          {:else}
            <li class="inline-flex items-center gap-1.5">
              <span class="shrink-0 text-black dark:text-white" aria-current="page">{crumb.label}</span>
            </li>
          {/if}
        {/each}
      {:else}
        <li class="inline-flex items-center gap-1.5">
          <span class="shrink-0 text-black dark:text-white" aria-current="page">{bucket}</span>
        </li>
      {/if}
    </ol>
  </nav>
  {#if !isSettings && bucketStats && (bucketStats.objectCount !== null || bucketStats.sizeBytes !== null)}
    <div
      class="flex w-full shrink-0 items-center gap-2 pl-6 text-sm text-muted-foreground sm:ml-auto sm:w-auto sm:pl-0"
      aria-label="Bucket statistics"
    >
      {#if bucketStats.objectCount !== null}
        <span class="tabular-nums">
          <span class="font-medium text-foreground">{bucketStats.objectCount.toLocaleString()}</span>
          {bucketStats.objectCount === 1 ? 'object' : 'objects'}
        </span>
      {/if}
      {#if bucketStats.objectCount !== null && bucketStats.sizeBytes !== null}
        <span class="text-neutral-300 dark:text-neutral-600" aria-hidden="true">·</span>
      {/if}
      {#if bucketStats.sizeBytes !== null}
        <span class="font-medium tabular-nums text-foreground">{formatBytes(bucketStats.sizeBytes)}</span>
      {/if}
    </div>
  {/if}
</div>

{@render children()}
