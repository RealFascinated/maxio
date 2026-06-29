<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import PolicyBuilder from './PolicyBuilder.svelte'
  import PolicyJsonPanel from './PolicyJsonPanel.svelte'
  import PolicyModeToggle from './PolicyModeToggle.svelte'
  import { listBuckets } from '$lib/api/buckets'
  import { bucketKeys } from '$lib/api/keys'
  import { parsePolicyDocument, serializePolicyDocument } from '$lib/policy/document'
  import { defaultBucketPolicy, defaultIdentityPolicy } from '$lib/policy/defaults'
  import type { PolicyDocument, PolicyEditorMode, PolicyVariant } from '$lib/policy/types'
  import { validatePolicyDocument, validatePolicyText, isPolicyValid, blockingPolicyIssue } from '$lib/policy/validation'
  import { toast } from '$lib/toast'

  interface Props {
    variant?: PolicyVariant
    value?: string
    readonly?: boolean
    /** Bucket context for bucket-policy validation (e.g. current bucket in settings). */
    buckets?: string[]
    onchange?: (value: string) => void
  }

  let {
    variant = 'identity',
    value = $bindable(''),
    readonly = false,
    buckets = [],
    onchange,
  }: Props = $props()

  const bucketsQuery = createQuery(() => ({
    queryKey: bucketKeys.list(),
    queryFn: listBuckets,
  }))

  const accessibleBuckets = $derived.by(() => {
    const fromApi = bucketsQuery.data?.buckets.map((b) => b.name) ?? []
    const seen = new Set<string>()
    const out: string[] = []
    for (const name of [...buckets, ...fromApi]) {
      if (!seen.has(name)) {
        seen.add(name)
        out.push(name)
      }
    }
    return out
  })

  const contextBucket = $derived(buckets[0])

  let mode = $state<PolicyEditorMode>('builder')
  let document = $state<PolicyDocument | null>(null)
  let jsonText = $state('')
  let jsonIssues = $state<ReturnType<typeof validatePolicyText>['issues']>([])
  let lastSyncedValue = $state('')

  function defaultForVariant(): string {
    if (variant === 'bucket' && contextBucket) {
      return defaultBucketPolicy(contextBucket)
    }
    return defaultIdentityPolicy()
  }

  function syncFromValue(text: string) {
    const trimmed = text.trim()
    const source = trimmed || defaultForVariant()
    const parsed = parsePolicyDocument(source)
    document = parsed
    jsonText = serializePolicyDocument(parsed, true)
    jsonIssues = validatePolicyDocument(parsed, variant, contextBucket)
  }

  $effect(() => {
    if (value === lastSyncedValue) return
    lastSyncedValue = value
    syncFromValue(value)
  })

  function emitDocument(doc: PolicyDocument) {
    document = doc
    const compact = serializePolicyDocument(doc)
    lastSyncedValue = compact
    value = compact
    jsonText = serializePolicyDocument(doc, true)
    jsonIssues = validatePolicyDocument(doc, variant, contextBucket)
    onchange?.(compact)
  }

  function switchMode(next: PolicyEditorMode) {
    if (next === mode) return
    if (next === 'builder') {
      const result = validatePolicyText(jsonText, variant, contextBucket)
      if (!isPolicyValid(result)) {
        jsonIssues = result.issues
        toast.error(blockingPolicyIssue(result.issues, 'Invalid policy JSON'))
        return
      }
      document = result.doc
      const compact = serializePolicyDocument(result.doc)
      lastSyncedValue = compact
      value = compact
      onchange?.(compact)
    } else if (document) {
      jsonText = serializePolicyDocument(document, true)
      jsonIssues = validatePolicyDocument(document, variant, contextBucket)
    }
    mode = next
  }

  function onJsonChange(text: string) {
    jsonText = text
    const result = validatePolicyText(text, variant, contextBucket)
    jsonIssues = result.issues
    if (result.doc && isPolicyValid(result)) {
      const compact = serializePolicyDocument(result.doc)
      lastSyncedValue = compact
      value = compact
      onchange?.(value)
    }
  }
</script>

<div class="space-y-4">
  {#if !readonly}
    <PolicyModeToggle {mode} {readonly} onchange={switchMode} />
  {/if}

  {#if mode === 'builder' && document}
    <PolicyBuilder
      bind:document
      {variant}
      {accessibleBuckets}
      {contextBucket}
      bucketsLoading={bucketsQuery.isPending}
      {readonly}
      onchange={emitDocument}
    />
  {:else}
    <PolicyJsonPanel bind:value={jsonText} {readonly} issues={jsonIssues} onchange={onJsonChange} />
  {/if}
</div>
