<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
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

<div class="space-y-3">
  {#if resources.length}
    <div class="flex flex-wrap gap-1.5">
      {#each resources as resource (resource)}
        <span
          class="inline-flex max-w-full items-center gap-1 rounded-sm border-2 border-border bg-muted/40 px-2 py-0.5 font-mono text-xs"
        >
          <span class="truncate" title={resource}>{resource}</span>
          {#if !readonly}
            <button
              type="button"
              class="shrink-0 text-muted-foreground hover:text-foreground"
              aria-label={`Remove ${resource}`}
              onclick={() => removeResource(resource)}
            >
              ×
            </button>
          {/if}
        </span>
      {/each}
    </div>
  {/if}

  {#if !readonly}
    <div class="space-y-2">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <Label class="text-xs font-medium uppercase tracking-wide text-muted-foreground">Your buckets</Label>
        <Button
          type="button"
          variant={resources.includes(ALL_BUCKETS_ARN) ? 'highlighted' : 'outline'}
          size="sm"
          onclick={() => toggleResource(ALL_BUCKETS_ARN)}
        >
          All buckets
        </Button>
      </div>

      {#if bucketsLoading}
        <p class="text-sm text-muted-foreground">Loading buckets…</p>
      {:else if accessibleBuckets.length === 0}
        <p class="text-sm text-muted-foreground">No buckets available.</p>
      {:else}
        <div class="max-h-48 overflow-y-auto rounded-sm border-2 border-border">
          {#each accessibleBuckets as name (name)}
            <div
              class="flex items-center justify-between gap-2 border-b border-border px-3 py-2 last:border-b-0"
            >
              <span class="min-w-0 truncate text-sm font-medium" title={name}>{name}</span>
              <div class="flex shrink-0 flex-wrap justify-end gap-1">
                <Button
                  type="button"
                  variant={hasBucketArn(name) ? 'highlighted' : 'outline'}
                  size="sm"
                  onclick={() => toggleResource(bucketArn(name))}
                >
                  Bucket
                </Button>
                <Button
                  type="button"
                  variant={hasObjectArn(name) ? 'highlighted' : 'outline'}
                  size="sm"
                  onclick={() => toggleResource(objectArn(name))}
                >
                  Objects
                </Button>
                <Button
                  type="button"
                  variant={hasBucketArn(name) && hasObjectArn(name) ? 'highlighted' : 'ghost'}
                  size="sm"
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
        bind:value={customResource}
        onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), addCustom())}
      />
      <Button type="button" variant="outline" size="sm" onclick={addCustom}>Add</Button>
    </div>
  {/if}
</div>
