<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import FileJson from 'lucide-svelte/icons/file-json'
  import MoreHorizontal from 'lucide-svelte/icons/ellipsis'
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
    columnHelper.accessor('policyId', { header: sortableHeader('ID') }),
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
      toast.success(`Policy ${data.name} created`)
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

  function closeCreateDialog() {
    showCreate = false
    newPolicyName = ''
    newPolicyDocument = defaultPolicyDocument
  }

  function closeViewerDialog() {
    policyViewer = null
    showViewerDialog = false
  }
</script>

<div class="flex flex-col gap-4">
  <PageHeader title="Policies">
    {#snippet actions()}
      <Button onclick={() => (showCreate = true)}><Plus />Add policy</Button>
    {/snippet}
  </PageHeader>

  {#if policiesQuery.isError}
    <Callout type="danger">
      {policiesQuery.error instanceof ApiError ? policiesQuery.error.message : 'Failed to load policies'}
    </Callout>
  {:else}
    <Toolbar>
      <Input
        type="search"
        placeholder="Filter"
        class="w-56"
        bind:value={searchInput}
        aria-label="Filter policies by name, ID or ARN"
      />
      {#snippet end()}
        <span class="num text-xs text-muted-foreground">{filteredPolicies.length} policies</span>
      {/snippet}
    </Toolbar>

    {#if policiesQuery.isPending}
      <div class="rounded-surface border border-border bg-panel">
        {#each [0, 1, 2] as n (n)}
          <div class="flex h-8 items-center border-b border-border px-3 last:border-b-0"><Skeleton class="w-1/3" /></div>
        {/each}
      </div>
    {:else if filteredPolicies.length === 0}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title={allPolicies.length === 0 ? 'No policies' : 'No matching policies'}>
          {#snippet action()}
            {#if allPolicies.length === 0}
              <Button size="sm" onclick={() => (showCreate = true)}><Plus />Add policy</Button>
            {/if}
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <Table.Root>
        <Table.Header>
          {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row class="hover:bg-transparent">
              {#each headerGroup.headers as header (header.id)}
                <Table.Head class={header.column.id === 'actions' ? 'w-8' : undefined}>
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
            <Table.Row
              class="cursor-pointer"
              tabindex={0}
              onclick={() => openPolicyViewer(policy)}
              onkeydown={(e: KeyboardEvent) => e.key === 'Enter' && openPolicyViewer(policy)}
            >
              <Table.Cell class="font-medium">{policy.name}</Table.Cell>
              <Table.Cell class="font-mono text-xs text-muted-foreground">{policy.policyId}</Table.Cell>
              <Table.Cell class="font-mono text-xs text-muted-foreground">{policy.arn}</Table.Cell>
              <Table.Cell>
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger>
                    {#snippet child({ props })}
                      <Button
                        {...props}
                        variant="ghost"
                        size="icon-xs"
                        aria-label={`Actions for ${policy.name}`}
                        onclick={(e: MouseEvent) => e.stopPropagation()}
                        onkeydown={(e: KeyboardEvent) => e.stopPropagation()}
                      >
                        <MoreHorizontal />
                      </Button>
                    {/snippet}
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content align="end">
                    <DropdownMenu.Item onSelect={() => openPolicyViewer(policy)}><FileJson />View document</DropdownMenu.Item>
                    <DropdownMenu.Item onSelect={() => copyText(policy.policyId, 'Policy ID')}><Copy />Copy ID</DropdownMenu.Item>
                    <DropdownMenu.Item onSelect={() => copyText(policy.arn, 'ARN')}><Copy />Copy ARN</DropdownMenu.Item>
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item variant="destructive" onSelect={() => (policyToDelete = policy.name)}>
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
  {/if}
</div>

<Dialog
  open={showCreate}
  title="Create policy"
  size="lg"
  loading={createPolicyMutation.isPending}
  onClose={closeCreateDialog}
>
  <form
    id="create-policy-form"
    onsubmit={(e) => {
      e.preventDefault()
      handleCreatePolicy()
    }}
    class="flex flex-col gap-4"
  >
    <div class="flex max-w-xs flex-col gap-1.5">
      <Label for="newPolicyName">Name</Label>
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
    <Button type="button" variant="secondary" disabled={createPolicyMutation.isPending} onclick={closeCreateDialog}>
      Cancel
    </Button>
    <Button
      type="submit"
      form="create-policy-form"
      loading={createPolicyMutation.isPending}
      disabled={!newPolicyName.trim()}
    >
      Create
    </Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={showViewerDialog}
  title={policyViewer?.name ?? ''}
  size="lg"
  onClose={closeViewerDialog}
>
  {#if policyViewer}
    <div class="flex flex-col gap-4">
      <div class="flex items-center gap-2">
        <span class="text-xs text-muted-foreground">ARN</span>
        <span class="min-w-0 flex-1 truncate font-mono text-xs">{policyViewer.arn}</span>
        <Button variant="secondary" size="sm" onclick={() => copyText(policyViewer!.arn, 'ARN')}>
          <Copy />Copy
        </Button>
      </div>
      <PolicyEditor variant="identity" value={policyViewer.document} readonly />
    </div>
  {/if}
  {#snippet footer()}
    {#if policyViewer}
      <Button variant="destructive" class="mr-auto" onclick={() => (policyToDelete = policyViewer!.name)}>
        Delete policy
      </Button>
      <Button variant="secondary" onclick={closeViewerDialog}>Close</Button>
    {/if}
  {/snippet}
</Dialog>

<ConfirmDialog
  open={policyToDelete !== null}
  title={policyToDelete ? `Delete policy ${policyToDelete}?` : 'Delete policy?'}
  description="Users with this policy attached lose its permissions."
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
