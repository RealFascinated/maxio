<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import Shield from 'lucide-svelte/icons/shield'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import FileJson from 'lucide-svelte/icons/file-json'
  import Search from 'lucide-svelte/icons/search'
  import X from 'lucide-svelte/icons/x'
  import Copy from 'lucide-svelte/icons/copy'
  import { toast, toastApiError } from '$lib/toast'
  import { policyKeys } from '$lib/api/keys'
  import {
    createPolicy,
    deletePolicy,
    getPolicy,
    listPolicies,
    type ManagedPolicySummary,
  } from '$lib/api/users'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { PolicyEditor } from '$lib/components/policy'
  import { defaultIdentityPolicy } from '$lib/policy/defaults'
  import { validatePolicyText, isPolicyValid, blockingPolicyIssue } from '$lib/policy/validation'
  import { jsonDocumentText, tryPrettyJson } from '$lib/preview'

  const defaultPolicyDocument = defaultIdentityPolicy()

  let showCreate = $state(false)
  let newPolicyName = $state('')
  let newPolicyDocument = $state(defaultPolicyDocument)
  let createPolicyInput = $state<HTMLInputElement | null>(null)
  let searchInput = $state('')
  let policyToDelete = $state<string | null>(null)
  let policyViewer = $state<{ name: string; arn: string; document: string } | null>(null)
  let showViewerDialog = $state(false)

  $effect(() => {
    if (showCreate && createPolicyInput) {
      queueMicrotask(() => createPolicyInput?.focus())
    }
  })

  const policiesQuery = createQuery(() => ({
    queryKey: policyKeys.list(),
    queryFn: listPolicies,
  }))

  const allPolicies = $derived(policiesQuery.data?.policies ?? [])
  const filteredPolicies = $derived.by(() => {
    const q = searchInput.trim().toLowerCase()
    if (!q) return allPolicies
    return allPolicies.filter(
      (policy) =>
        policy.name.toLowerCase().includes(q) ||
        policy.policyId.toLowerCase().includes(q) ||
        policy.arn.toLowerCase().includes(q),
    )
  })

  const columnHelper = createColumnHelper<typeof sortableTableFeatures, ManagedPolicySummary>()
  const columns = [
    columnHelper.accessor('name', { header: sortableHeader('Name') }),
    columnHelper.accessor('policyId', { header: sortableHeader('Policy ID') }),
    columnHelper.accessor('arn', { header: sortableHeader('ARN') }),
    columnHelper.display({ id: 'actions', enableSorting: false, header: '' }),
  ]

  const table = createTable({
    features: sortableTableFeatures,
    columns: columns as never,
    get data() {
      return filteredPolicies
    },
  })

  const createPolicyMutation = createMutation(() => ({
    mutationFn: ({ name, document }: { name: string; document: string }) =>
      createPolicy(name, document),
    onSuccess: (data) => {
      toast.success(`Policy "${data.name}" created`)
      newPolicyName = ''
      newPolicyDocument = defaultPolicyDocument
      showCreate = false
      queryClient.invalidateQueries({ queryKey: policyKeys.list() })
    },
  }))

  const deletePolicyMutation = createMutation(() => ({
    mutationFn: deletePolicy,
    onSuccess: () => {
      toast.success('Policy deleted')
      closeViewerDialog()
      queryClient.invalidateQueries({ queryKey: policyKeys.list() })
    },
  }))

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast.success(`${label} copied`)
    } catch (err) {
      console.error('copyText failed:', err)
      toast.error('Failed to copy')
    }
  }

  async function handleCreatePolicy() {
    const name = newPolicyName.trim()
    if (!name) return
    const validation = validatePolicyText(newPolicyDocument, 'identity')
    if (!isPolicyValid(validation)) {
      toast.error(blockingPolicyIssue(validation.issues, 'Policy document must be valid JSON'))
      return
    }
    try {
      await createPolicyMutation.mutateAsync({ name, document: newPolicyDocument })
    } catch (err) {
      console.error('createPolicy failed:', err)
      toastApiError(err, 'Failed to create policy')
    }
  }

  async function openPolicyViewer(policy: ManagedPolicySummary) {
    try {
      const detail = await getPolicy(policy.name)
      const raw = jsonDocumentText(detail.document)
      policyViewer = {
        name: detail.name,
        arn: detail.arn,
        document: tryPrettyJson(raw) ?? raw,
      }
      showViewerDialog = true
    } catch (err) {
      console.error('getPolicy failed:', err)
      toastApiError(err, 'Failed to load policy')
    }
  }

  function closeViewerDialog() {
    policyViewer = null
    showViewerDialog = false
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between gap-4">
    <div class="flex items-center gap-2">
      <Shield class="size-5 text-coollabs dark:text-warning" />
      <h1>Managed policies</h1>
    </div>
    <Button variant="brand" onclick={() => (showCreate = true)}>
      <Plus class="size-4" />
      Add policy
    </Button>
  </div>

  {#if policiesQuery.isError}
    <Callout type="danger">
      {policiesQuery.error instanceof ApiError ? policiesQuery.error.message : 'Failed to load policies'}
    </Callout>
  {:else if policiesQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading policies…</p>
  {:else if allPolicies.length === 0}
    <Callout type="info">
      <div class="flex flex-col gap-3">
        <span class="inline-flex items-center gap-2">
          <Shield class="size-4 opacity-70" />
          No managed policies yet — create one to attach reusable permissions to IAM users.
        </span>
        <Button variant="brand" class="w-fit" onclick={() => (showCreate = true)}>
          <Plus class="size-4" />
          Add policy
        </Button>
      </div>
    </Callout>
  {:else}
    <div class="relative max-w-sm">
      <Search class="pointer-events-none absolute left-2 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        type="search"
        placeholder="Filter policies…"
        class="h-8 pl-8 pr-8"
        bind:value={searchInput}
        aria-label="Filter policies by name, ID, or ARN"
      />
      {#if searchInput}
        <button
          type="button"
          class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground transition-colors hover:text-foreground"
          onclick={() => (searchInput = '')}
          aria-label="Clear filter"
        >
          <X class="size-4" />
        </button>
      {/if}
    </div>

    {#if filteredPolicies.length === 0}
      <Callout type="info">
        <span class="inline-flex items-center gap-2">
          <Search class="size-4 opacity-70" />
          No policies matching &ldquo;{searchInput.trim()}&rdquo;.
        </span>
      </Callout>
    {:else}
      <Table.Root>
        <Table.Header>
          {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row>
              {#each headerGroup.headers as header (header.id)}
                <Table.Head class={header.column.id === 'actions' ? 'w-20' : undefined}>
                  {#if !header.isPlaceholder}
                    <FlexRender header={header} />
                  {/if}
                </Table.Head>
              {/each}
            </Table.Row>
          {/each}
        </Table.Header>
        <Table.Body>
          {#each table.getRowModel().rows as row (row.id)}
            {@const policy = row.original}
            <Table.Row>
              <Table.Cell class="font-medium">{policy.name}</Table.Cell>
              <Table.Cell>
                <div class="flex items-center gap-1">
                  <span
                    class="max-w-36 truncate font-mono text-xs text-muted-foreground"
                    title={policy.policyId}
                  >{policy.policyId}</span>
                  <button
                    type="button"
                    class="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                    title="Copy policy ID"
                    aria-label="Copy policy ID"
                    onclick={() => copyText(policy.policyId, 'Policy ID')}
                  >
                    <Copy class="size-3.5" />
                  </button>
                </div>
              </Table.Cell>
              <Table.Cell>
                <div class="flex items-center gap-1">
                  <span
                    class="max-w-64 truncate font-mono text-xs text-muted-foreground"
                    title={policy.arn}
                  >{policy.arn}</span>
                  <button
                    type="button"
                    class="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                    title="Copy ARN"
                    aria-label="Copy ARN"
                    onclick={() => copyText(policy.arn, 'ARN')}
                  >
                    <Copy class="size-3.5" />
                  </button>
                </div>
              </Table.Cell>
              <Table.Cell>
                <div class="flex items-center justify-end gap-3">
                  <button
                    type="button"
                    class="text-muted-foreground transition-colors hover:text-foreground"
                    title="View policy document"
                    aria-label="View policy document"
                    onclick={() => openPolicyViewer(policy)}
                  >
                    <FileJson class="size-4" />
                  </button>
                  <button
                    type="button"
                    class="text-muted-foreground transition-colors hover:text-destructive"
                    title="Delete policy"
                    aria-label="Delete policy"
                    onclick={() => (policyToDelete = policy.name)}
                  >
                    <Trash2 class="size-4" />
                  </button>
                </div>
              </Table.Cell>
            </Table.Row>
          {/each}
        </Table.Body>
      </Table.Root>
    {/if}
  {/if}
</div>

<Dialog
  open={showCreate}
  title="Create managed policy"
  size="lg"
  loading={createPolicyMutation.isPending}
  onClose={() => {
    showCreate = false
    newPolicyName = ''
    newPolicyDocument = defaultPolicyDocument
  }}
>
  <form
    id="create-policy-form"
    onsubmit={(e) => {
      e.preventDefault()
      handleCreatePolicy()
    }}
    class="space-y-4"
  >
    <div class="space-y-1.5">
      <Label for="newPolicyName">Policy name</Label>
      <Input
        bind:ref={createPolicyInput}
        id="newPolicyName"
        type="text"
        bind:value={newPolicyName}
        placeholder="read-only-access"
        disabled={createPolicyMutation.isPending}
      />
    </div>
    <PolicyEditor variant="identity" bind:value={newPolicyDocument} />
  </form>
  {#snippet footer()}
    <Button
      type="button"
      variant="default"
      disabled={createPolicyMutation.isPending}
      onclick={() => {
        showCreate = false
        newPolicyName = ''
        newPolicyDocument = defaultPolicyDocument
      }}
    >
      Cancel
    </Button>
    <Button
      type="submit"
      form="create-policy-form"
      variant="highlighted"
      disabled={createPolicyMutation.isPending || !newPolicyName.trim()}
    >
      {createPolicyMutation.isPending ? 'Creating…' : 'Create policy'}
    </Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={showViewerDialog}
  title="Managed policy"
  size="lg"
  onClose={closeViewerDialog}
>
  {#if policyViewer}
    <div class="space-y-4">
      <div class="space-y-1.5">
        <Label>Policy name</Label>
        <p class="font-medium">{policyViewer.name}</p>
      </div>
      <div class="space-y-1.5">
        <Label>ARN</Label>
        <div class="flex items-center gap-2">
          <p class="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground" title={policyViewer.arn}>
            {policyViewer.arn}
          </p>
          <Button variant="outline" size="sm" onclick={() => copyText(policyViewer!.arn, 'ARN')}>
            <Copy class="size-3.5" />
            Copy
          </Button>
        </div>
      </div>
      <PolicyEditor variant="identity" value={policyViewer.document} readonly />
    </div>
  {/if}
  {#snippet footer()}
    {#if policyViewer}
      <div class="flex w-full flex-wrap items-center justify-between gap-2">
        <Button
          variant="destructive"
          onclick={() => (policyToDelete = policyViewer!.name)}
        >
          Delete policy
        </Button>
        <Button variant="brand" onclick={closeViewerDialog}>Close</Button>
      </div>
    {/if}
  {/snippet}
</Dialog>

<ConfirmDialog
  open={policyToDelete !== null}
  title="Delete managed policy?"
  description={policyToDelete
    ? `Remove policy "${policyToDelete}"? Users with this policy attached will lose those permissions.`
    : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deletePolicyMutation.isPending}
  onConfirm={async () => {
    if (!policyToDelete) return
    try {
      await deletePolicyMutation.mutateAsync(policyToDelete)
    } catch (err) {
      console.error('deletePolicy failed:', err)
      toastApiError(err, 'Failed to delete policy')
    } finally {
      policyToDelete = null
    }
  }}
  onClose={() => (policyToDelete = null)}
/>
