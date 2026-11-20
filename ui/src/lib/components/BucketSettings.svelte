<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { toast, toastApiError } from '$lib/toast'
  import MoreHorizontal from 'lucide-svelte/icons/ellipsis'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import CompressionJobPanel from '$lib/components/CompressionJobPanel.svelte'
  import { Callout } from '$lib/components/ui/callout'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Switch } from '$lib/components/ui/switch'
  import * as Table from '$lib/components/ui/table'
  import { Tabs } from '$lib/components/ui/tabs'
  import { goto } from '$app/navigation'
  import { routes } from '$lib/navigation'
  import { deleteBucket } from '$lib/api/buckets'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { bucketKeys, settingsKeys } from '$lib/api/keys'
  import { PolicyEditor } from '$lib/components/policy'
  import {
    deleteBucketPolicy,
    getBucketPolicy,
    getCors,
    getLifecycle,
    getPublicAccess,
    getCompression,
    getVersioning,
    putBucketPolicy,
    setCors,
    setLifecycle,
    setPublicAccess,
    setCompression,
    setVersioning,
    type LifecycleRule,
  } from '$lib/api/settings'
  import { defaultBucketPolicy } from '$lib/policy/defaults'
  import { validatePolicyText, isPolicyValid, blockingPolicyIssue } from '$lib/policy/validation'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'

  interface Props {
    bucket: string
  }
  let { bucket }: Props = $props()
  let pendingConfirmation = $state<{
    title: string
    description: string
    confirmLabel: string
    destructive?: boolean
    action: () => Promise<void>
  } | null>(null)

  const versioningQuery = createQuery(() => ({
    queryKey: settingsKeys.versioning(bucket),
    queryFn: () => getVersioning(bucket),
  }))
  const compressionQuery = createQuery(() => ({
    queryKey: settingsKeys.compression(bucket),
    queryFn: () => getCompression(bucket),
  }))
  const publicQuery = createQuery(() => ({
    queryKey: settingsKeys.publicAccess(bucket),
    queryFn: () => getPublicAccess(bucket),
  }))
  const corsQuery = createQuery(() => ({
    queryKey: settingsKeys.cors(bucket),
    queryFn: () => getCors(bucket),
  }))
  const lifecycleQuery = createQuery(() => ({
    queryKey: settingsKeys.lifecycle(bucket),
    queryFn: () => getLifecycle(bucket),
    // Only read on the Lifecycle tab; tracked props would miss the pending to success change.
    notifyOnChangeProps: 'all' as const,
  }))
  const policyQuery = createQuery(() => ({
    queryKey: settingsKeys.policy(bucket),
    queryFn: () => getBucketPolicy(bucket),
  }))

  let policyDocument = $state('')
  let policyDirty = $state(false)
  let addingStatement = $state(false)
  let activeTab = $state('general')
  let deleteBucketOpen = $state(false)
  const tabs = [
    { value: 'general', label: 'General' },
    { value: 'policy', label: 'Policy' },
    { value: 'lifecycle', label: 'Lifecycle' },
  ]
  const hasPolicy = $derived(!!policyQuery.data?.document)

  $effect(() => {
    if (policyDirty || policyQuery.isPending) return
    const doc = policyQuery.data?.document
    policyDocument = doc ? doc : defaultBucketPolicy(bucket)
  })

  let newRuleId = $state('')
  let newRulePrefix = $state('')
  let newExpireDays = $state('')
  let newNoncurrentDays = $state('')

  const lifecycleRules = $derived(lifecycleQuery.data?.rules ?? [])

  const versioningEnabled = $derived(!!versioningQuery.data?.enabled)
  const compressionEnabled = $derived(!!compressionQuery.data?.enabled)
  const publicRead = $derived(!!publicQuery.data?.read)
  const publicList = $derived(!!publicQuery.data?.list)
  const corsEnabled = $derived(!!corsQuery.data?.enabled)

  const versioningMutation = createMutation(() => ({
    mutationFn: (enabled: boolean) => setVersioning(bucket, enabled),
    onSuccess: (_data, enabled) => {
      toast.success(enabled ? 'Versioning enabled' : 'Versioning disabled')
      queryClient.invalidateQueries({ queryKey: settingsKeys.versioning(bucket) })
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
    },
  }))

  const compressionMutation = createMutation(() => ({
    mutationFn: (enabled: boolean) => setCompression(bucket, enabled),
    onSuccess: (_data, enabled) => {
      toast.success(enabled ? 'Compression enabled' : 'Compression disabled')
      queryClient.invalidateQueries({ queryKey: settingsKeys.compression(bucket) })
    },
  }))

  const publicMutation = createMutation(() => ({
    mutationFn: (next: { read: boolean; list: boolean }) => setPublicAccess(bucket, next.read, next.list),
    onSuccess: (_data, next) => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.publicAccess(bucket) })
      queryClient.invalidateQueries({ queryKey: settingsKeys.policy(bucket) })
      policyDirty = false
      toast.success(next.read !== publicRead ? (next.read ? 'Public read enabled' : 'Public read disabled') : (next.list ? 'Public listing enabled' : 'Public listing disabled'))
    },
  }))

  const corsMutation = createMutation(() => ({
    mutationFn: (enabled: boolean) => setCors(bucket, enabled),
    onSuccess: (_data, enabled) => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.cors(bucket) })
      toast.success(enabled ? 'Cross-origin read enabled' : 'Cross-origin read disabled')
    },
  }))

  const deleteBucketMutation = createMutation(() => ({
    mutationFn: () => deleteBucket(bucket),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: bucketKeys.list() })
      toast.success(`Bucket ${bucket} deleted`)
      goto(routes.home())
    },
  }))

  async function removeBucket() {
    try {
      await deleteBucketMutation.mutateAsync()
    } catch (err) {
      console.error('deleteBucket failed:', err)
      toastApiError(err, 'Failed to delete bucket')
    }
  }

  const lifecycleMutation = createMutation(() => ({
    mutationFn: (rules: LifecycleRule[]) => setLifecycle(bucket, rules),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.lifecycle(bucket) })
      toast.success('Lifecycle rules updated')
    },
  }))

  const policyMutation = createMutation(() => ({
    mutationFn: (document: string) => putBucketPolicy(bucket, document),
    onSuccess: () => {
      policyDirty = false
      queryClient.invalidateQueries({ queryKey: settingsKeys.policy(bucket) })
      queryClient.invalidateQueries({ queryKey: settingsKeys.publicAccess(bucket) })
      toast.success('Bucket policy saved')
    },
  }))

  const deletePolicyMutation = createMutation(() => ({
    mutationFn: () => deleteBucketPolicy(bucket),
    onSuccess: () => {
      policyDirty = false
      addingStatement = false
      policyDocument = defaultBucketPolicy(bucket)
      queryClient.invalidateQueries({ queryKey: settingsKeys.policy(bucket) })
      queryClient.invalidateQueries({ queryKey: settingsKeys.publicAccess(bucket) })
      toast.success('Bucket policy deleted')
    },
  }))

  async function saveBucketPolicy() {
    const validation = validatePolicyText(policyDocument, 'bucket', bucket)
    if (!isPolicyValid(validation)) {
      toast.error(blockingPolicyIssue(validation.issues, 'Invalid bucket policy'))
      return
    }
    try {
      await policyMutation.mutateAsync(policyDocument)
    } catch (err) {
      console.error('saveBucketPolicy failed:', err)
      toastApiError(err, 'Failed to save bucket policy')
    }
  }

  function requestDeleteBucketPolicy() {
    pendingConfirmation = {
      title: 'Delete bucket policy?',
      description: 'Public access settings stop applying until a new policy is saved.',
      confirmLabel: 'Delete policy',
      destructive: true,
      action: async () => {
        try {
          await deletePolicyMutation.mutateAsync()
          pendingConfirmation = null
        } catch (err) {
          console.error('deleteBucketPolicy failed:', err)
          toastApiError(err, 'Failed to delete bucket policy')
        }
      },
    }
  }

  function onPolicyChange(value: string) {
    policyDocument = value
    policyDirty = true
  }

  function formatRuleActions(rule: LifecycleRule): string {
    const parts: string[] = []
    for (const action of rule.actions) {
      if (action.type === 'expire_objects') {
        parts.push(`expire current after ${action.days}d`)
      } else if (action.type === 'noncurrent_version_expiration') {
        parts.push(`expire old versions after ${action.noncurrent_days}d`)
      }
    }
    return parts.join('; ')
  }

  async function saveLifecycleRules(rules: LifecycleRule[]) {
    try {
      await lifecycleMutation.mutateAsync(rules)
      pendingConfirmation = null
    } catch (err) {
      console.error('saveLifecycleRules failed:', err)
      toastApiError(err, 'Failed to update lifecycle rules')
    }
  }

  async function toggleLifecycleRule(rule: LifecycleRule) {
    const rules = lifecycleRules.map((r) =>
      r.id === rule.id ? { ...r, enabled: !r.enabled } : r,
    )
    await saveLifecycleRules(rules)
  }

  function requestDeleteLifecycleRule(rule: LifecycleRule) {
    pendingConfirmation = {
      title: 'Delete lifecycle rule?',
      description: `Rule ${rule.id} will be removed.`,
      confirmLabel: 'Delete rule',
      destructive: true,
      action: async () => {
        await saveLifecycleRules(lifecycleRules.filter((r) => r.id !== rule.id))
      },
    }
  }

  async function addLifecycleRule() {
    const id = newRuleId.trim()
    const expireDays = newExpireDays.trim() ? Number(newExpireDays) : null
    const noncurrentDays = newNoncurrentDays.trim() ? Number(newNoncurrentDays) : null

    if (!id) {
      toast.error('Rule ID is required')
      return
    }
    if (lifecycleRules.some((r) => r.id === id)) {
      toast.error('A rule with this ID already exists')
      return
    }
    if ((!expireDays || expireDays < 1) && (!noncurrentDays || noncurrentDays < 1)) {
      toast.error('Set at least one expiration period (minimum 1 day)')
      return
    }

    const actions: LifecycleRule['actions'] = []
    if (expireDays && expireDays >= 1) {
      actions.push({ type: 'expire_objects', days: expireDays })
    }
    if (noncurrentDays && noncurrentDays >= 1) {
      actions.push({ type: 'noncurrent_version_expiration', noncurrent_days: noncurrentDays })
    }

    const rule: LifecycleRule = {
      id,
      enabled: true,
      prefix: newRulePrefix.trim() || undefined,
      actions,
    }

    await saveLifecycleRules([...lifecycleRules, rule])
    newRuleId = ''
    newRulePrefix = ''
    newExpireDays = ''
    newNoncurrentDays = ''
  }

  async function toggleVersioning() {
    const newState = !versioningEnabled
    if (versioningEnabled && !newState) {
      pendingConfirmation = {
        title: 'Disable versioning?',
        description: 'All non-current versions are deleted permanently. Only the latest version of each object is kept.',
        confirmLabel: 'Disable versioning',
        destructive: true,
        action: () => applyVersioning(newState),
      }
      return
    }
    await applyVersioning(newState)
  }

  async function applyVersioning(newState: boolean) {
    try {
      await versioningMutation.mutateAsync(newState)
      pendingConfirmation = null
    } catch (err) {
      console.error('toggleVersioning failed:', err)
      toastApiError(err, 'Failed to update versioning')
    }
  }

  async function toggleCompression() {
    try {
      await compressionMutation.mutateAsync(!compressionEnabled)
    } catch (err) {
      console.error('toggleCompression failed:', err)
      toastApiError(err, 'Failed to update compression')
    }
  }

  async function togglePublicRead() {
    const newState = !publicRead
    if (newState) {
      pendingConfirmation = {
        title: 'Enable public read?',
        description: 'Anyone with an object URL can download it without credentials.',
        confirmLabel: 'Enable public read',
        action: () => applyPublicAccess({ read: newState, list: publicList }),
      }
      return
    }
    await applyPublicAccess({ read: newState, list: publicList })
  }

  async function applyPublicAccess(next: { read: boolean; list: boolean }) {
    try {
      await publicMutation.mutateAsync(next)
      pendingConfirmation = null
    } catch (err) {
      console.error('togglePublicRead failed:', err)
      toastApiError(err, 'Failed to update public access')
    }
  }

  async function toggleCors() {
    const newState = !corsEnabled
    if (newState) {
      pendingConfirmation = {
        title: 'Enable cross-origin read?',
        description: 'Browsers on any website can fetch objects from this bucket.',
        confirmLabel: 'Enable cross-origin read',
        action: () => applyCors(newState),
      }
      return
    }
    await applyCors(newState)
  }

  async function applyCors(newState: boolean) {
    try {
      await corsMutation.mutateAsync(newState)
      pendingConfirmation = null
    } catch (err) {
      console.error('toggleCors failed:', err)
      toastApiError(err, 'Failed to update CORS')
    }
  }

  async function togglePublicList() {
    const newState = !publicList
    if (newState) {
      pendingConfirmation = {
        title: 'Enable public listing?',
        description: 'Anyone can list every object key in this bucket without credentials.',
        confirmLabel: 'Enable public listing',
        action: () => applyPublicAccess({ read: publicRead, list: newState }),
      }
      return
    }
    await applyPublicAccess({ read: publicRead, list: newState })
  }
</script>

{#snippet toggleRow(
  label: string,
  checked: boolean,
  pending: boolean,
  busy: boolean,
  onclick: () => void,
  flagged = false,
)}
  <div class="flex h-10 items-center justify-between gap-4 px-3">
    <span class="flex items-center gap-2">
      {label}
      {#if flagged}<Badge variant="warning" label="Public" />{/if}
    </span>
    {#if pending}
      <Skeleton class="h-4 w-7" />
    {:else}
      <Switch {checked} {onclick} disabled={busy} aria-label={label} />
    {/if}
  </div>
{/snippet}

<div class="flex max-w-2xl flex-col gap-4">
  {#if versioningQuery.isError || compressionQuery.isError || publicQuery.isError || corsQuery.isError || lifecycleQuery.isError || policyQuery.isError}
    <Callout type="danger">Failed to load bucket settings</Callout>
  {/if}

  <Tabs bind:value={activeTab} items={tabs} aria-label="Settings section" />

  {#if activeTab === 'general'}
    <div class="divide-y divide-border rounded-surface border border-border bg-panel">
      {@render toggleRow('Versioning', versioningEnabled, versioningQuery.isPending, versioningMutation.isPending, toggleVersioning)}
      {@render toggleRow('LZ4 compression', compressionEnabled, compressionQuery.isPending, compressionMutation.isPending, toggleCompression)}
      {@render toggleRow('Public read', publicRead, publicQuery.isPending, publicMutation.isPending, togglePublicRead, publicRead)}
      {@render toggleRow('Public listing', publicList, publicQuery.isPending, publicMutation.isPending, togglePublicList, publicList)}
      {@render toggleRow('Cross-origin read', corsEnabled, corsQuery.isPending, corsMutation.isPending, toggleCors)}
    </div>

    <CompressionJobPanel {bucket} {compressionEnabled} />

    <h2 class="mt-2">Danger zone</h2>
    <div class="flex h-10 items-center justify-between gap-4 rounded-surface border border-border bg-panel px-3">
      <span>Delete bucket</span>
      <Button variant="destructive" size="sm" onclick={() => (deleteBucketOpen = true)}>Delete bucket</Button>
    </div>
  {/if}

  {#if activeTab === 'policy'}
    {#if policyQuery.isPending}
      <div class="flex flex-col gap-2 rounded-surface border border-border bg-panel p-3">
        <Skeleton class="w-1/3" />
        <Skeleton class="w-full" />
        <Skeleton class="w-2/3" />
      </div>
    {:else if !hasPolicy && !policyDirty && !addingStatement}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title="No policy">
          {#snippet action()}
            <Button size="sm" onclick={() => (addingStatement = true)}><Plus />Add statement</Button>
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <PolicyEditor
        variant="bucket"
        buckets={[bucket]}
        value={policyDocument}
        onchange={onPolicyChange}
      />
      <div class="flex items-center justify-between gap-2">
        {#if policyQuery.data?.document}
          <Button
            variant="destructive"
            size="sm"
            onclick={requestDeleteBucketPolicy}
            disabled={deletePolicyMutation.isPending}
          >
            Delete policy
          </Button>
        {:else}
          <span></span>
        {/if}
        <Button
          size="sm"
          onclick={saveBucketPolicy}
          loading={policyMutation.isPending}
          disabled={!policyDirty}
        >
          Save policy
        </Button>
      </div>
    {/if}
  {/if}

  {#if activeTab === 'lifecycle'}
    {#if lifecycleQuery.isPending}
      <div class="flex flex-col gap-2 rounded-surface border border-border bg-panel p-3">
        <Skeleton class="w-1/3" />
        <Skeleton class="w-full" />
      </div>
    {:else if lifecycleRules.length === 0}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title="No lifecycle rules" />
      </div>
    {:else}
      <Table.Root>
        <Table.Header>
          <Table.Row class="hover:bg-transparent">
            <Table.Head>Rule</Table.Head>
            <Table.Head>Prefix</Table.Head>
            <Table.Head>Actions</Table.Head>
            <Table.Head class="w-12">On</Table.Head>
            <Table.Head class="w-8"></Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each lifecycleRules as rule (rule.id)}
            <Table.Row>
              <Table.Cell class="font-medium">{rule.id}</Table.Cell>
              <Table.Cell class="font-mono text-xs text-muted-foreground">{rule.prefix ?? ''}</Table.Cell>
              <Table.Cell class="num text-muted-foreground">{formatRuleActions(rule)}</Table.Cell>
              <Table.Cell>
                <Switch
                  checked={rule.enabled}
                  onclick={() => toggleLifecycleRule(rule)}
                  disabled={lifecycleMutation.isPending}
                  aria-label={`Toggle rule ${rule.id}`}
                />
              </Table.Cell>
              <Table.Cell>
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger>
                    {#snippet child({ props })}
                      <Button {...props} variant="ghost" size="icon-xs" aria-label={`Actions for rule ${rule.id}`}>
                        <MoreHorizontal />
                      </Button>
                    {/snippet}
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content align="end">
                    <DropdownMenu.Item
                      variant="destructive"
                      disabled={lifecycleMutation.isPending}
                      onSelect={() => requestDeleteLifecycleRule(rule)}
                    >
                      <Trash2 />Delete
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    {/if}

    <h2 class="mt-2">Add rule</h2>
    <form
      class="grid grid-cols-2 gap-3 rounded-surface border border-border bg-panel p-3 sm:grid-cols-4"
      onsubmit={(e) => {
        e.preventDefault()
        addLifecycleRule()
      }}
    >
      <div class="flex flex-col gap-1.5">
        <Label for="lifecycle-rule-id">Rule ID</Label>
        <Input id="lifecycle-rule-id" bind:value={newRuleId} />
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="lifecycle-prefix">Prefix</Label>
        <Input id="lifecycle-prefix" class="font-mono text-xs" bind:value={newRulePrefix} />
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="lifecycle-expire">Expire current (days)</Label>
        <Input id="lifecycle-expire" class="num" inputmode="numeric" bind:value={newExpireDays} />
      </div>
      <div class="flex flex-col gap-1.5">
        <Label for="lifecycle-noncurrent">Expire old versions (days)</Label>
        <Input id="lifecycle-noncurrent" class="num" inputmode="numeric" bind:value={newNoncurrentDays} />
      </div>
      <div class="col-span-2 flex justify-end sm:col-span-4">
        <Button type="submit" size="sm" disabled={lifecycleMutation.isPending}>Add rule</Button>
      </div>
    </form>
  {/if}
</div>

<ConfirmDialog
  bind:open={deleteBucketOpen}
  title="Delete bucket?"
  description={`${bucket} must be empty.`}
  confirmLabel="Delete bucket"
  confirmVariant="destructive"
  confirmationText={bucket}
  confirmationLabel="Bucket name"
  loading={deleteBucketMutation.isPending}
  onConfirm={removeBucket}
/>

{#if pendingConfirmation}
  <ConfirmDialog
    open
    title={pendingConfirmation.title}
    description={pendingConfirmation.description}
    confirmLabel={pendingConfirmation.confirmLabel}
    confirmVariant={pendingConfirmation.destructive ? 'destructive' : 'default'}
    loading={versioningMutation.isPending || publicMutation.isPending || corsMutation.isPending || lifecycleMutation.isPending || policyMutation.isPending || deletePolicyMutation.isPending}
    onClose={() => (pendingConfirmation = null)}
    onConfirm={pendingConfirmation.action}
  />
{/if}
