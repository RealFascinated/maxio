<script lang="ts">
  import X from 'lucide-svelte/icons/x'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Tag } from '$lib/components/ui/tag'
  import { ALL_BUCKETS_ARN, bucketArn, objectArn } from '$lib/policy/resources'

  interface Props {
    resources: string[]
    accessibleBuckets?: string[]
    bucketsLoading?: boolean
    readonly?: boolean
    onchange?: (resources: string[]) => void
  }

  let {
    resources = $bindable([]),
    accessibleBuckets = [],
    bucketsLoading = false,
    readonly = false,
    onchange,
  }: Props = $props()

  let customResource = $state('')

  function addResource(arn: string) {
    if (readonly || !arn || resources.includes(arn)) return
    resources = [...resources, arn]
    onchange?.(resources)
  }

  function toggleResource(arn: string) {
    if (readonly || !arn) return
    if (resources.includes(arn)) {
      removeResource(arn)
    } else {
      addResource(arn)
    }
  }

  function toggleBucketPair(name: string) {
    if (readonly) return
    const pair = [bucketArn(name), objectArn(name)]
    const bothSelected = pair.every((arn) => resources.includes(arn))
    if (bothSelected) {
      resources = resources.filter((r) => !pair.includes(r))
    } else {
      const next = [...resources]
      for (const arn of pair) {
        if (!next.includes(arn)) next.push(arn)
      }
      resources = next
    }
    onchange?.(resources)
  }

  function removeResource(arn: string) {
    if (readonly) return
    resources = resources.filter((r) => r !== arn)
    onchange?.(resources)
  }

  function addCustom() {
    const trimmed = customResource.trim()
    if (!trimmed) return
    addResource(trimmed)
    customResource = ''
  }

  function hasBucketArn(name: string): boolean {
    return resources.includes(bucketArn(name))
  }

  function hasObjectArn(name: string): boolean {
    return resources.includes(objectArn(name))
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  {#if resources.length}
    <div class="flex flex-wrap gap-1.5">
      {#each resources as resource (resource)}
        <Tag class="h-auto min-h-5 max-w-full gap-1 py-0.5 font-mono break-all">
          {resource}
          {#if !readonly}
            <button
              type="button"
              class="inline-flex size-4 shrink-0 items-center justify-center rounded-control hover:text-foreground"
              aria-label={`Remove resource ${resource}`}
              onclick={() => removeResource(resource)}
            >
              <X class="size-3" />
            </button>
          {/if}
        </Tag>
      {/each}
    </div>
  {/if}

  {#if !readonly}
    <div class="flex flex-col">
      <div class="flex h-8 items-center justify-between gap-2 border-b border-border">
        <span class="text-xs font-medium text-muted-foreground">Buckets</span>
        <Button
          type="button"
          variant={resources.includes(ALL_BUCKETS_ARN) ? 'default' : 'outline'}
          size="xs"
          onclick={() => toggleResource(ALL_BUCKETS_ARN)}
        >
          All buckets
        </Button>
      </div>

      {#if bucketsLoading}
        <div class="flex flex-col gap-2 py-2">
          <Skeleton class="w-1/3" />
          <Skeleton class="w-1/2" />
        </div>
      {:else if accessibleBuckets.length === 0}
        <p class="py-2 text-sm text-muted-foreground">No buckets</p>
      {:else}
        <div class="max-h-48 divide-y divide-border overflow-y-auto">
          {#each accessibleBuckets as name (name)}
            <div class="flex h-8 items-center justify-between gap-2">
              <span class="min-w-0 truncate text-sm">{name}</span>
              <div class="flex shrink-0 gap-1">
                <Button
                  type="button"
                  variant={hasBucketArn(name) ? 'default' : 'outline'}
                  size="xs"
                  onclick={() => toggleResource(bucketArn(name))}
                >
                  Bucket
                </Button>
                <Button
                  type="button"
                  variant={hasObjectArn(name) ? 'default' : 'outline'}
                  size="xs"
                  onclick={() => toggleResource(objectArn(name))}
                >
                  Objects
                </Button>
                <Button
                  type="button"
                  variant={hasBucketArn(name) && hasObjectArn(name) ? 'default' : 'outline'}
                  size="xs"
                  onclick={() => toggleBucketPair(name)}
                >
                  Both
                </Button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="flex gap-2">
      <Input
        type="text"
        placeholder="arn:aws:s3:::bucket/*"
        class="font-mono text-xs"
        aria-label="Custom resource"
        bind:value={customResource}
        onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), addCustom())}
      />
      <Button type="button" variant="secondary" onclick={addCustom}>Add</Button>
    </div>
  {/if}
</div>
