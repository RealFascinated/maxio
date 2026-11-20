<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import { createColumnHelper, createTable, FlexRender } from '@tanstack/svelte-table'
  import * as Table from '$lib/components/ui/table'
  import { sortableHeader, sortableTableFeatures } from '$lib/table/sortable'
  import { Button } from '$lib/components/ui/button'
  import { Badge } from '$lib/components/ui/badge'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { PageHeader } from '$lib/components/ui/page-header'
  import { Select } from '$lib/components/ui/select'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { SlideOver } from '$lib/components/ui/slide-over'
  import { Tag } from '$lib/components/ui/tag'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import KeyRound from 'lucide-svelte/icons/key-round'
  import Shield from 'lucide-svelte/icons/shield'
  import FileJson from 'lucide-svelte/icons/file-json'
  import Pencil from 'lucide-svelte/icons/pencil'
  import Unlink from 'lucide-svelte/icons/unlink'
  import Download from 'lucide-svelte/icons/download'
  import MoreHorizontal from 'lucide-svelte/icons/ellipsis'
  import Copy from 'lucide-svelte/icons/copy'
  import type { LucideIcon } from '$lib/lucide'
  import { toast, toastApiError } from '$lib/toast'
  import { userKeys, policyKeys } from '$lib/api/keys'
  import {
    attachUserPolicy,
    createUser,
    createUserKey,
    deleteUser,
    deleteUserKey,
    deleteUserPolicy,
    detachUserPolicy,
    getUserPolicy,
    listPolicies,
    listUsers,
    putUserPolicy,
    type IamUserSummary,
  } from '$lib/api/users'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { formatDate } from '$lib/format'
  import { PolicyEditor } from '$lib/components/policy'
  import { defaultIdentityPolicy } from '$lib/policy/defaults'
  import { validatePolicyText, isPolicyValid, blockingPolicyIssue } from '$lib/policy/validation'
  import { jsonDocumentText, tryPrettyJson } from '$lib/preview'

  let showCreate = $state(false)
  let newUsername = $state('')
  let createUserInput = $state<HTMLInputElement | null>(null)
  let searchInput = $state('')
  let selectedUsername = $state<string | null>(null)
  let userToDelete = $state<string | null>(null)
  let keyToDelete = $state<{ username: string; accessKeyId: string } | null>(null)
  let keyToCreate = $state<string | null>(null)
  let newKeySecret = $state<{ username: string; accessKeyId: string; secretAccessKey: string } | null>(null)
  let policyEditor = $state<{ username: string; policyName: string; document: string } | null>(null)
  let policyToDelete = $state<{ username: string; policyName: string } | null>(null)
  let attachPolicyUser = $state<IamUserSummary | null>(null)
  let selectedPolicyArn = $state('')
  let detachPolicyTarget = $state<{ username: string; policyArn: string; policyName: string } | null>(null)
  let showKeySecretDialog = $state(false)
  let keySecretAcknowledged = $state(false)
  let showPolicyDialog = $state(false)
  let showAttachDialog = $state(false)

  $effect(() => {
    if (showCreate && createUserInput) {
      queueMicrotask(() => createUserInput?.focus())
    }
  })

  const usersQuery = createQuery(() => ({
    queryKey: userKeys.list(),
    queryFn: listUsers,
  }))

  const policiesQuery = createQuery(() => ({
    queryKey: policyKeys.list(),
    queryFn: listPolicies,
    enabled: showAttachDialog,
  }))

  const attachablePolicies = $derived.by(() => {
    if (!attachPolicyUser) return []
    const attached = new Set(attachPolicyUser.attachedPolicies)
    return (policiesQuery.data?.policies ?? []).filter((policy) => !attached.has(policy.arn))
  })

  const allUsers = $derived(usersQuery.data?.users ?? [])
  const selectedUser = $derived(allUsers.find((user) => user.username === selectedUsername) ?? null)
  const filteredUsers = $derived.by(() => {
    const q = searchInput.trim().toLowerCase()
    if (!q) return allUsers
    return allUsers.filter(
      (user) =>
        user.username.toLowerCase().includes(q) ||
        user.userId.toLowerCase().includes(q) ||
        user.accessKeys.some((key) => key.accessKeyId.toLowerCase().includes(q)),
    )
  })

  const columnHelper = createColumnHelper<typeof sortableTableFeatures, IamUserSummary>()
  const columns = [
    columnHelper.accessor('username', { header: sortableHeader('Username') }),
    columnHelper.accessor((user) => user.inlinePolicies.length + user.attachedPolicies.length, {
      id: 'policies',
      header: sortableHeader('Policies'),
    }),
    columnHelper.accessor((user) => user.accessKeys.length, {
      id: 'accessKeys',
      header: sortableHeader('Keys'),
    }),
    columnHelper.accessor('createdAt', { header: sortableHeader('Created') }),
    columnHelper.display({ id: 'actions', enableSorting: false, header: '' }),
  ]

  const columnClass: Record<string, string> = {
    policies: 'hidden md:table-cell',
    accessKeys: 'text-right',
    createdAt: 'hidden sm:table-cell',
    actions: 'w-10',
  }

  const table = createTable({
    features: sortableTableFeatures,
    columns: columns as never,
    get data() {
      return filteredUsers
    },
  })

  const createUserMutation = createMutation(() => ({
    mutationFn: createUser,
    onSuccess: (data) => {
      toast.success(`User "${data.username}" created`)
      if (data.accessKey) {
        newKeySecret = {
          username: data.username,
          accessKeyId: data.accessKey.accessKeyId,
          secretAccessKey: data.accessKey.secretAccessKey,
        }
        keySecretAcknowledged = false
        showKeySecretDialog = true
      }
      newUsername = ''
      showCreate = false
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const createKeyMutation = createMutation(() => ({
    mutationFn: ({ username }: { username: string }) => createUserKey(username),
    onSuccess: (data, { username }) => {
      newKeySecret = {
        username,
        accessKeyId: data.accessKeyId,
        secretAccessKey: data.secretAccessKey,
      }
      keySecretAcknowledged = false
      showKeySecretDialog = true
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const deleteUserMutation = createMutation(() => ({
    mutationFn: deleteUser,
    onSuccess: () => {
      toast.success('User deleted')
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const deleteKeyMutation = createMutation(() => ({
    mutationFn: ({ username, accessKeyId }: { username: string; accessKeyId: string }) =>
      deleteUserKey(username, accessKeyId),
    onSuccess: () => {
      toast.success('Access key deleted')
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const savePolicyMutation = createMutation(() => ({
    mutationFn: ({
      username,
      policyName,
      document,
    }: {
      username: string
      policyName: string
      document: string
    }) => putUserPolicy(username, policyName, document),
    onSuccess: () => {
      toast.success('Policy saved')
      closePolicyDialog()
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const deletePolicyMutation = createMutation(() => ({
    mutationFn: ({ username, policyName }: { username: string; policyName: string }) =>
      deleteUserPolicy(username, policyName),
    onSuccess: () => {
      toast.success('Policy deleted')
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const attachPolicyMutation = createMutation(() => ({
    mutationFn: ({ username, policyArn }: { username: string; policyArn: string }) =>
      attachUserPolicy(username, policyArn),
    onSuccess: () => {
      toast.success('Policy attached')
      closeAttachDialog()
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
    },
  }))

  const detachPolicyMutation = createMutation(() => ({
    mutationFn: ({ username, policyArn }: { username: string; policyArn: string }) =>
      detachUserPolicy(username, policyArn),
    onSuccess: () => {
      toast.success('Policy detached')
      queryClient.invalidateQueries({ queryKey: userKeys.list() })
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

  function downloadKeyCsv(key: { username: string; accessKeyId: string; secretAccessKey: string }) {
    const blob = new Blob([`accessKey,secretKey\n${key.accessKeyId},${key.secretAccessKey}\n`], {
      type: 'text/csv',
    })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `${key.username}-${key.accessKeyId}.csv`
    link.click()
    URL.revokeObjectURL(url)
    keySecretAcknowledged = true
  }

  async function handleCreateUser() {
    const username = newUsername.trim()
    if (!username) return
    try {
      await createUserMutation.mutateAsync(username)
    } catch (err) {
      console.error('createUser failed:', err)
      toastApiError(err, 'Failed to create user')
    }
  }

  async function openPolicyEditor(user: IamUserSummary, policyName: string) {
    try {
      const existing = await getUserPolicy(user.username, policyName)
      const raw = jsonDocumentText(existing.document)
      policyEditor = {
        username: user.username,
        policyName,
        document: tryPrettyJson(raw) ?? raw,
      }
      showPolicyDialog = true
    } catch (err) {
      console.error('getUserPolicy failed:', err)
      toastApiError(err, 'Failed to load policy')
    }
  }

  function closePolicyDialog() {
    policyEditor = null
    showPolicyDialog = false
  }

  function openNewPolicyEditor(user: IamUserSummary) {
    policyEditor = {
      username: user.username,
      policyName: 'inline-policy',
      document: defaultIdentityPolicy(),
    }
    showPolicyDialog = true
  }

  function openPolicyEditorForUser(user: IamUserSummary) {
    if (user.inlinePolicies.length > 0) {
      openPolicyEditor(user, user.inlinePolicies[0])
    } else {
      openNewPolicyEditor(user)
    }
  }

  function openAttachDialog(user: IamUserSummary) {
    attachPolicyUser = user
    selectedPolicyArn = ''
    showAttachDialog = true
  }

  function closeAttachDialog() {
    attachPolicyUser = null
    selectedPolicyArn = ''
    showAttachDialog = false
  }

  function managedPolicyName(arn: string): string {
    return arn.split('/').pop() ?? arn
  }

  const POLICY_TAGS_SHOWN = 2

  function policyNames(user: IamUserSummary): string[] {
    return [...user.inlinePolicies, ...user.attachedPolicies.map(managedPolicyName)]
  }

  async function handleAttachPolicy() {
    if (!attachPolicyUser || !selectedPolicyArn) return
    try {
      await attachPolicyMutation.mutateAsync({
        username: attachPolicyUser.username,
        policyArn: selectedPolicyArn,
      })
    } catch (err) {
      console.error('attachUserPolicy failed:', err)
      toastApiError(err, 'Failed to attach policy')
    }
  }

  async function savePolicy() {
    if (!policyEditor) return
    const validation = validatePolicyText(policyEditor.document, 'identity')
    if (!isPolicyValid(validation)) {
      toast.error(blockingPolicyIssue(validation.issues, 'Policy document must be valid JSON'))
      return
    }
    try {
      await savePolicyMutation.mutateAsync(policyEditor)
    } catch (err) {
      console.error('putUserPolicy failed:', err)
      toastApiError(err, 'Failed to save policy')
    }
  }
</script>

{#snippet iconAction(label: string, Icon: LucideIcon, onclick: () => void, danger = false)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-xs"
          class={danger ? 'hover:text-danger' : undefined}
          aria-label={label}
          {onclick}
        >
          <Icon />
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<div class="flex flex-col gap-3">
  <PageHeader title="Users">
    {#snippet actions()}
      <Button onclick={() => (showCreate = true)}><Plus />Create user</Button>
    {/snippet}
  </PageHeader>

  {#if usersQuery.isError}
    <Callout type="danger">
      {usersQuery.error instanceof ApiError ? usersQuery.error.message : 'Failed to load users'}
    </Callout>
  {:else if !usersQuery.isPending && allUsers.length === 0}
    <div class="rounded-surface border border-border bg-panel">
      <EmptyState title="No users">
        {#snippet action()}
          <Button size="sm" onclick={() => (showCreate = true)}><Plus />Create user</Button>
        {/snippet}
      </EmptyState>
    </div>
  {:else}
    <Toolbar>
      <Input
        placeholder="Filter"
        class="w-56 max-w-full"
        bind:value={searchInput}
        aria-label="Filter users by name, ID or access key"
      />
      {#snippet end()}
        {#if !usersQuery.isPending}
          <span class="num text-xs text-muted-foreground">
            {filteredUsers.length === allUsers.length
              ? allUsers.length
              : `${filteredUsers.length} of ${allUsers.length}`}
            {allUsers.length === 1 ? 'user' : 'users'}
          </span>
        {/if}
      {/snippet}
    </Toolbar>

    {#if !usersQuery.isPending && filteredUsers.length === 0}
      <div class="rounded-surface border border-border bg-panel">
        <EmptyState title="No matching users" />
      </div>
    {:else}
      <Table.Root>
        <Table.Header>
          {#each table.getHeaderGroups() as headerGroup (headerGroup.id)}
            <Table.Row class="hover:bg-transparent">
              {#each headerGroup.headers as header (header.id)}
                <Table.Head class={columnClass[header.column.id]}>
                  {#if !header.isPlaceholder}
                    <FlexRender header={header} />
                  {/if}
                </Table.Head>
              {/each}
            </Table.Row>
          {/each}
        </Table.Header>
        <Table.Body>
          {#if usersQuery.isPending}
            {#each Array(4) as _, index (index)}
              <Table.Row class="hover:bg-transparent">
                <Table.Cell><Skeleton class="w-32" /></Table.Cell>
                <Table.Cell class={columnClass.policies}><Skeleton class="w-40" /></Table.Cell>
                <Table.Cell><Skeleton class="ml-auto w-6" /></Table.Cell>
                <Table.Cell class={columnClass.createdAt}><Skeleton class="w-28" /></Table.Cell>
                <Table.Cell></Table.Cell>
              </Table.Row>
            {/each}
          {:else}
            {#each table.getRowModel().rows as row (row.id)}
              {@const user = row.original}
              {@const names = policyNames(user)}
              <Table.Row
                class="cursor-pointer"
                data-state={selectedUsername === user.username ? 'selected' : undefined}
                onclick={() => (selectedUsername = user.username)}
              >
                <Table.Cell class="font-medium">
                  <button
                    type="button"
                    class="rounded-control hover:underline"
                    onclick={() => (selectedUsername = user.username)}
                  >{user.username}</button>
                </Table.Cell>
                <Table.Cell class={columnClass.policies}>
                  <div class="flex items-center gap-1">
                    {#each names.slice(0, POLICY_TAGS_SHOWN) as name (name)}
                      <Tag class="max-w-40 font-mono"><span class="truncate">{name}</span></Tag>
                    {/each}
                    {#if names.length > POLICY_TAGS_SHOWN}
                      <span class="num text-xs text-muted-foreground">+{names.length - POLICY_TAGS_SHOWN}</span>
                    {/if}
                  </div>
                </Table.Cell>
                <Table.Cell class="num text-right text-muted-foreground">{user.accessKeys.length}</Table.Cell>
                <Table.Cell class="num text-muted-foreground {columnClass.createdAt}">
                  {formatDate(user.createdAt)}
                </Table.Cell>
                <Table.Cell class="w-10 pr-2 pl-0 text-right">
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          variant="ghost"
                          size="icon-xs"
                          aria-label="Actions for {user.username}"
                          onclick={(e: Event) => e.stopPropagation()}
                        ><MoreHorizontal /></Button>
                      {/snippet}
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Content align="end">
                      <DropdownMenu.Item onSelect={() => (keyToCreate = user.username)}>
                        <KeyRound />Create access key
                      </DropdownMenu.Item>
                      <DropdownMenu.Item onSelect={() => openAttachDialog(user)}>
                        <Shield />Attach policy
                      </DropdownMenu.Item>
                      <DropdownMenu.Item onSelect={() => openPolicyEditorForUser(user)}>
                        <FileJson />{user.inlinePolicies.length > 0 ? 'Edit inline policy' : 'Add inline policy'}
                      </DropdownMenu.Item>
                      <DropdownMenu.Item onSelect={() => copyText(user.userId, 'User ID')}>
                        <Copy />Copy user ID
                      </DropdownMenu.Item>
                      <DropdownMenu.Separator />
                      <DropdownMenu.Item variant="destructive" onSelect={() => (userToDelete = user.username)}>
                        <Trash2 />Delete user
                      </DropdownMenu.Item>
                    </DropdownMenu.Content>
                  </DropdownMenu.Root>
                </Table.Cell>
              </Table.Row>
            {/each}
          {/if}
        </Table.Body>
      </Table.Root>
    {/if}
  {/if}
</div>

<SlideOver
  open={selectedUser !== null}
  title={selectedUser?.username ?? ''}
  onClose={() => (selectedUsername = null)}
>
  {#if selectedUser}
    {@const user = selectedUser}
    <div class="flex flex-col gap-6">
      <dl class="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-1">
        <dt class="text-muted-foreground">User ID</dt>
        <dd class="flex min-w-0 items-center gap-1">
          <span class="truncate font-mono text-xs">{user.userId}</span>
          {@render iconAction('Copy user ID', Copy, () => copyText(user.userId, 'User ID'))}
        </dd>
        <dt class="text-muted-foreground">Created</dt>
        <dd class="num">{formatDate(user.createdAt)}</dd>
      </dl>

      <section class="flex flex-col gap-2">
        <div class="flex min-h-6 items-center justify-between gap-2">
          <h2 class="text-sm">Access keys</h2>
          <Button variant="secondary" size="xs" onclick={() => (keyToCreate = user.username)}>
            <Plus />Create key
          </Button>
        </div>
        {#if user.accessKeys.length === 0}
          <div class="rounded-surface border border-border"><EmptyState title="No access keys" class="py-6" /></div>
        {:else}
          <ul class="rounded-surface border border-border">
            {#each user.accessKeys as key (key.accessKeyId)}
              <li class="flex h-8 items-center gap-2 border-b border-border pr-1 pl-3 last:border-b-0">
                <span class="min-w-0 flex-1 truncate font-mono text-xs">{key.accessKeyId}</span>
                <Badge
                  variant={key.status === 'Active' ? 'success' : 'neutral'}
                  label={key.status}
                />
                {@render iconAction('Copy access key ID', Copy, () => copyText(key.accessKeyId, 'Access key ID'))}
                {@render iconAction(
                  'Delete access key',
                  Trash2,
                  () => (keyToDelete = { username: user.username, accessKeyId: key.accessKeyId }),
                  true,
                )}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="flex flex-col gap-2">
        <div class="flex min-h-6 items-center justify-between gap-2">
          <h2 class="text-sm">Policies</h2>
          <div class="flex items-center gap-2">
            <Button variant="secondary" size="xs" onclick={() => openAttachDialog(user)}>
              <Shield />Attach
            </Button>
            <Button variant="secondary" size="xs" onclick={() => openNewPolicyEditor(user)}>
              <Plus />Inline policy
            </Button>
          </div>
        </div>
        {#if user.inlinePolicies.length === 0 && user.attachedPolicies.length === 0}
          <div class="rounded-surface border border-border"><EmptyState title="No policies" class="py-6" /></div>
        {:else}
          <ul class="rounded-surface border border-border">
            {#each user.inlinePolicies as policyName (policyName)}
              <li class="flex h-8 items-center gap-2 border-b border-border pr-1 pl-3 last:border-b-0">
                <span class="min-w-0 flex-1 truncate font-mono text-xs">{policyName}</span>
                <span class="text-xs text-muted-foreground">Inline</span>
                {@render iconAction('Edit inline policy', Pencil, () => openPolicyEditor(user, policyName))}
              </li>
            {/each}
            {#each user.attachedPolicies as arn (arn)}
              <li class="flex h-8 items-center gap-2 border-b border-border pr-1 pl-3 last:border-b-0">
                <span class="min-w-0 flex-1 truncate font-mono text-xs">{managedPolicyName(arn)}</span>
                <span class="text-xs text-muted-foreground">Managed</span>
                {@render iconAction(
                  'Detach policy',
                  Unlink,
                  () =>
                    (detachPolicyTarget = {
                      username: user.username,
                      policyArn: arn,
                      policyName: managedPolicyName(arn),
                    }),
                  true,
                )}
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    </div>
  {/if}
  {#snippet footer()}
    {#if selectedUser}
      {@const user = selectedUser}
      <Button variant="destructive" class="mr-auto" onclick={() => (userToDelete = user.username)}>
        <Trash2 />Delete user
      </Button>
    {/if}
    <Button variant="secondary" onclick={() => (selectedUsername = null)}>Close</Button>
  {/snippet}
</SlideOver>

<Dialog
  open={showCreate}
  title="Create user"
  loading={createUserMutation.isPending}
  onClose={() => { showCreate = false; newUsername = '' }}
>
  <form
    id="create-user-form"
    onsubmit={(e) => { e.preventDefault(); handleCreateUser() }}
    class="flex flex-col gap-1.5"
  >
    <Label for="newUsername">Username</Label>
    <Input
      bind:ref={createUserInput}
      id="newUsername"
      type="text"
      bind:value={newUsername}
      placeholder="alice"
      autocomplete="off"
      disabled={createUserMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button
      variant="secondary"
      disabled={createUserMutation.isPending}
      onclick={() => { showCreate = false; newUsername = '' }}
    >
      Cancel
    </Button>
    <Button
      type="submit"
      form="create-user-form"
      loading={createUserMutation.isPending}
      disabled={!newUsername.trim()}
    >
      Create
    </Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={showKeySecretDialog}
  title="New access key"
  onClose={() => { newKeySecret = null; showKeySecretDialog = false }}
>
  {#if newKeySecret}
    <div class="flex flex-col gap-3">
      <Callout type="warning">Secret is shown once</Callout>
      {@render secretRow('Access key', newKeySecret.accessKeyId, () =>
        copyText(newKeySecret!.accessKeyId, 'Access key'),
      )}
      {@render secretRow('Secret key', newKeySecret.secretAccessKey, async () => {
        await copyText(newKeySecret!.secretAccessKey, 'Secret key')
        keySecretAcknowledged = true
      })}
    </div>
  {/if}
  {#snippet footer()}
    {#if newKeySecret}
      <Button variant="secondary" onclick={() => downloadKeyCsv(newKeySecret!)}><Download />Download .csv</Button>
    {/if}
    <Button
      disabled={!keySecretAcknowledged}
      onclick={() => { newKeySecret = null; showKeySecretDialog = false }}
    >
      Done
    </Button>
  {/snippet}
</Dialog>

{#snippet secretRow(label: string, value: string, oncopy: () => void)}
  <div class="flex flex-col gap-1.5">
    <Label>{label}</Label>
    <div class="flex items-center gap-2">
      <code class="min-w-0 flex-1 rounded-control border border-border-strong bg-inset px-2 py-1 font-mono text-xs break-all">{value}</code>
      <Button variant="secondary" size="xs" onclick={oncopy}><Copy />Copy</Button>
    </div>
  </div>
{/snippet}

<Dialog
  bind:open={showPolicyDialog}
  title="Inline policy"
  size="lg"
  loading={savePolicyMutation.isPending}
  onClose={closePolicyDialog}
>
  {#if policyEditor}
    <div class="flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <Label for="policyName">Policy name</Label>
        <Input id="policyName" bind:value={policyEditor.policyName} />
      </div>
      <PolicyEditor variant="identity" bind:value={policyEditor.document} />
    </div>
  {/if}
  {#snippet footer()}
    {#if policyEditor}
      <div class="flex w-full flex-wrap items-center justify-between gap-2">
        {#if policyEditor.policyName}
          <Button
            variant="destructive"
            disabled={savePolicyMutation.isPending}
            onclick={() =>
              (policyToDelete = {
                username: policyEditor!.username,
                policyName: policyEditor!.policyName,
              })}
          >
            Delete policy
          </Button>
        {:else}
          <span></span>
        {/if}
        <div class="flex gap-2">
          <Button variant="secondary" disabled={savePolicyMutation.isPending} onclick={closePolicyDialog}>
            Cancel
          </Button>
          <Button loading={savePolicyMutation.isPending} onclick={savePolicy}>Save</Button>
        </div>
      </div>
    {/if}
  {/snippet}
</Dialog>

<ConfirmDialog
  open={userToDelete !== null}
  title="Delete user {userToDelete ?? ''}"
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deleteUserMutation.isPending}
  onConfirm={async () => {
    if (!userToDelete) return
    try {
      await deleteUserMutation.mutateAsync(userToDelete)
      if (selectedUsername === userToDelete) selectedUsername = null
    } catch (err) {
      console.error('deleteUser failed:', err)
      toastApiError(err, 'Failed to delete user')
    } finally {
      userToDelete = null
    }
  }}
  onClose={() => (userToDelete = null)}
/>

<ConfirmDialog
  open={keyToCreate !== null}
  title="Create access key for {keyToCreate ?? ''}"
  confirmLabel="Create"
  loading={createKeyMutation.isPending}
  onConfirm={async () => {
    if (!keyToCreate) return
    const username = keyToCreate
    try {
      await createKeyMutation.mutateAsync({ username })
    } catch (err) {
      console.error('createUserKey failed:', err)
      toastApiError(err, 'Failed to create access key')
    } finally {
      keyToCreate = null
    }
  }}
  onClose={() => (keyToCreate = null)}
/>

<ConfirmDialog
  open={keyToDelete !== null}
  title="Delete access key"
  description={keyToDelete?.accessKeyId ?? ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deleteKeyMutation.isPending}
  onConfirm={async () => {
    if (!keyToDelete) return
    try {
      await deleteKeyMutation.mutateAsync(keyToDelete)
    } catch (err) {
      console.error('deleteUserKey failed:', err)
      toastApiError(err, 'Failed to delete key')
    } finally {
      keyToDelete = null
    }
  }}
  onClose={() => (keyToDelete = null)}
/>

<ConfirmDialog
  open={policyToDelete !== null}
  title="Delete inline policy"
  description={policyToDelete?.policyName ?? ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deletePolicyMutation.isPending}
  onConfirm={async () => {
    if (!policyToDelete) return
    try {
      await deletePolicyMutation.mutateAsync(policyToDelete)
      closePolicyDialog()
    } catch (err) {
      console.error('deleteUserPolicy failed:', err)
      toastApiError(err, 'Failed to delete policy')
    } finally {
      policyToDelete = null
    }
  }}
  onClose={() => (policyToDelete = null)}
/>

<Dialog
  bind:open={showAttachDialog}
  title={attachPolicyUser ? `Attach policy to ${attachPolicyUser.username}` : 'Attach policy'}
  loading={attachPolicyMutation.isPending}
  onClose={closeAttachDialog}
>
  {#if attachPolicyUser}
    {#if policiesQuery.isPending}
      <Skeleton class="h-7 w-full" />
    {:else if policiesQuery.isError}
      <Callout type="danger">
        {policiesQuery.error instanceof ApiError ? policiesQuery.error.message : 'Failed to load policies'}
      </Callout>
    {:else if attachablePolicies.length === 0}
      <p class="text-muted-foreground">No managed policies to attach</p>
    {:else}
      <div class="flex flex-col gap-1.5">
        <Label>Policy</Label>
        <Select
          bind:value={selectedPolicyArn}
          aria-label="Policy"
          placeholder="Select a policy"
          disabled={attachPolicyMutation.isPending}
          options={attachablePolicies.map((policy) => ({ value: policy.arn, label: policy.name }))}
        />
      </div>
    {/if}
  {/if}
  {#snippet footer()}
    <Button variant="secondary" disabled={attachPolicyMutation.isPending} onclick={closeAttachDialog}>
      Cancel
    </Button>
    <Button
      loading={attachPolicyMutation.isPending}
      disabled={!selectedPolicyArn}
      onclick={handleAttachPolicy}
    >
      Attach
    </Button>
  {/snippet}
</Dialog>

<ConfirmDialog
  open={detachPolicyTarget !== null}
  title="Detach {detachPolicyTarget?.policyName ?? ''} from {detachPolicyTarget?.username ?? ''}"
  confirmLabel="Detach"
  confirmVariant="destructive"
  loading={detachPolicyMutation.isPending}
  onConfirm={async () => {
    if (!detachPolicyTarget) return
    try {
      await detachPolicyMutation.mutateAsync({
        username: detachPolicyTarget.username,
        policyArn: detachPolicyTarget.policyArn,
      })
    } catch (err) {
      console.error('detachUserPolicy failed:', err)
      toastApiError(err, 'Failed to detach policy')
    } finally {
      detachPolicyTarget = null
    }
  }}
  onClose={() => (detachPolicyTarget = null)}
/>
