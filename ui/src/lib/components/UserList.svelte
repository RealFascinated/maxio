<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
  import { Badge } from '$lib/components/ui/badge'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import Users from 'lucide-svelte/icons/users'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import KeyRound from 'lucide-svelte/icons/key-round'
  import FileJson from 'lucide-svelte/icons/file-json'
  import Search from 'lucide-svelte/icons/search'
  import X from 'lucide-svelte/icons/x'
  import Copy from 'lucide-svelte/icons/copy'
  import { toast } from '$lib/toast'
  import { userKeys } from '$lib/api/keys'
  import {
    createUser,
    createUserKey,
    deleteUser,
    deleteUserKey,
    deleteUserPolicy,
    getUserPolicy,
    listUsers,
    putUserPolicy,
    type IamUserSummary,
  } from '$lib/api/users'
  import { ApiError } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'
  import { formatDate } from '$lib/format'

  let showCreate = $state(false)
  let newUsername = $state('')
  let createUserInput = $state<HTMLInputElement | null>(null)
  let searchInput = $state('')
  let userToDelete = $state<string | null>(null)
  let keyToDelete = $state<{ username: string; accessKeyId: string } | null>(null)
  let keyToCreate = $state<string | null>(null)
  let newKeySecret = $state<{ username: string; accessKeyId: string; secretAccessKey: string } | null>(null)
  let policyEditor = $state<{ username: string; policyName: string; document: string } | null>(null)
  let policyToDelete = $state<{ username: string; policyName: string } | null>(null)
  let showKeySecretDialog = $state(false)
  let showPolicyDialog = $state(false)

  $effect(() => {
    if (showCreate && createUserInput) {
      queueMicrotask(() => createUserInput?.focus())
    }
  })

  const usersQuery = createQuery(() => ({
    queryKey: userKeys.list(),
    queryFn: listUsers,
  }))

  const allUsers = $derived(usersQuery.data?.users ?? [])
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
      policyEditor = null
      showPolicyDialog = false
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

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast.success(`${label} copied`)
    } catch (err) {
      console.error('copyText failed:', err)
      toast.error('Failed to copy')
    }
  }

  async function handleCreateUser() {
    const username = newUsername.trim()
    if (!username) return
    try {
      await createUserMutation.mutateAsync(username)
    } catch (err) {
      console.error('createUser failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to create user')
    }
  }

  async function openPolicyEditor(user: IamUserSummary, policyName: string) {
    try {
      const existing = await getUserPolicy(user.username, policyName)
      policyEditor = {
        username: user.username,
        policyName,
        document: existing.document,
      }
      showPolicyDialog = true
    } catch (err) {
      console.error('getUserPolicy failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to load policy')
    }
  }

  function openNewPolicyEditor(user: IamUserSummary) {
    policyEditor = {
      username: user.username,
      policyName: 'inline-policy',
      document: JSON.stringify(
        {
          Version: '2012-10-17',
          Statement: [
            {
              Effect: 'Allow',
              Action: ['s3:ListBucket', 's3:GetObject'],
              Resource: ['arn:aws:s3:::my-bucket', 'arn:aws:s3:::my-bucket/*'],
            },
          ],
        },
        null,
        2,
      ),
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

  async function savePolicy() {
    if (!policyEditor) return
    try {
      JSON.parse(policyEditor.document)
    } catch {
      toast.error('Policy document must be valid JSON')
      return
    }
    try {
      await savePolicyMutation.mutateAsync(policyEditor)
    } catch (err) {
      console.error('putUserPolicy failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to save policy')
    }
  }
</script>

<div class="space-y-6">
  <div class="flex items-center justify-between gap-4">
    <div class="flex items-center gap-2">
      <Users class="size-5 text-coollabs dark:text-warning" />
      <h2 class="text-lg font-semibold">IAM Users</h2>
    </div>
    <Button variant="brand" onclick={() => (showCreate = true)}>
      <Plus class="size-4" />
      Add user
    </Button>
  </div>

  {#if usersQuery.isError}
    <Callout type="danger">
      {usersQuery.error instanceof ApiError ? usersQuery.error.message : 'Failed to load users'}
    </Callout>
  {:else if usersQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading users…</p>
  {:else if allUsers.length === 0}
    <Callout type="info">
      <div class="flex flex-col gap-3">
        <span class="inline-flex items-center gap-2">
          <Users class="size-4 opacity-70" />
          No IAM users yet — create one to grant scoped console and S3 access.
        </span>
        <Button variant="brand" class="w-fit" onclick={() => (showCreate = true)}>
          <Plus class="size-4" />
          Add user
        </Button>
      </div>
    </Callout>
  {:else}
    <div class="relative max-w-sm">
      <Search class="pointer-events-none absolute left-2 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input
        type="search"
        placeholder="Filter users…"
        class="h-8 pl-8 pr-8"
        bind:value={searchInput}
        aria-label="Filter users by username, ID, or access key"
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

    {#if filteredUsers.length === 0}
      <Callout type="info">
        <span class="inline-flex items-center gap-2">
          <Search class="size-4 opacity-70" />
          No users matching &ldquo;{searchInput.trim()}&rdquo;.
        </span>
      </Callout>
    {:else}
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <Table.Head>Username</Table.Head>
            <Table.Head>User ID</Table.Head>
            <Table.Head>Access keys</Table.Head>
            <Table.Head>Policies</Table.Head>
            <Table.Head>Created</Table.Head>
            <Table.Head class="w-28"></Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each filteredUsers as user (user.username)}
            <Table.Row>
              <Table.Cell class="font-medium">{user.username}</Table.Cell>
              <Table.Cell>
                <div class="flex items-center gap-1">
                  <span
                    class="max-w-36 truncate font-mono text-xs text-muted-foreground"
                    title={user.userId}
                  >{user.userId}</span>
                  <button
                    type="button"
                    class="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                    title="Copy user ID"
                    aria-label="Copy user ID"
                    onclick={() => copyText(user.userId, 'User ID')}
                  >
                    <Copy class="size-3.5" />
                  </button>
                </div>
              </Table.Cell>
              <Table.Cell>
                {#if user.accessKeys.length === 0}
                  <span class="text-xs text-muted-foreground">None</span>
                {:else}
                  <div class="flex flex-col gap-1.5">
                    {#each user.accessKeys as key (key.accessKeyId)}
                      <div class="flex flex-wrap items-center gap-1.5">
                        <span class="font-mono text-xs">{key.accessKeyId}</span>
                        {#if key.status === 'Active'}
                          <Badge variant="success" label="Active" />
                        {:else}
                          <span class="text-xs text-muted-foreground">{key.status}</span>
                        {/if}
                        <button
                          type="button"
                          class="text-muted-foreground transition-colors hover:text-foreground"
                          title="Copy access key ID"
                          aria-label="Copy access key ID"
                          onclick={() => copyText(key.accessKeyId, 'Access key ID')}
                        >
                          <Copy class="size-3.5" />
                        </button>
                        <button
                          type="button"
                          class="text-muted-foreground transition-colors hover:text-destructive"
                          title="Delete access key"
                          aria-label="Delete access key"
                          onclick={() =>
                            (keyToDelete = { username: user.username, accessKeyId: key.accessKeyId })}
                        >
                          <Trash2 class="size-3.5" />
                        </button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </Table.Cell>
              <Table.Cell>
                {#if user.inlinePolicies.length === 0 && user.attachedPolicies.length === 0}
                  <span class="text-xs text-muted-foreground">None</span>
                {:else}
                  <div class="flex flex-wrap gap-1">
                    {#each user.inlinePolicies as policyName}
                      <button
                        type="button"
                        class="inline-flex items-center gap-1 rounded-sm bg-neutral-100 px-2 py-0.5 font-mono text-xs transition-colors hover:bg-neutral-200 dark:bg-coolgray-200 dark:hover:bg-coolgray-300"
                        onclick={() => openPolicyEditor(user, policyName)}
                      >
                        <FileJson class="size-3 opacity-70" />
                        {policyName}
                      </button>
                    {/each}
                    {#each user.attachedPolicies as arn}
                      <span
                        class="rounded-sm border border-neutral-200 px-2 py-0.5 font-mono text-xs text-muted-foreground dark:border-coolgray-300"
                        title={arn}
                      >{arn.split('/').pop()}</span>
                    {/each}
                  </div>
                {/if}
              </Table.Cell>
              <Table.Cell class="text-muted-foreground">{formatDate(user.createdAt)}</Table.Cell>
              <Table.Cell>
                <div class="flex items-center justify-end gap-3">
                  <button
                    type="button"
                    class="text-muted-foreground transition-colors hover:text-foreground"
                    title="Create access key"
                    aria-label="Create access key"
                    onclick={() => (keyToCreate = user.username)}
                  >
                    <KeyRound class="size-4" />
                  </button>
                  <button
                    type="button"
                    class="text-muted-foreground transition-colors hover:text-foreground"
                    title={user.inlinePolicies.length > 0 ? 'Edit inline policy' : 'Add inline policy'}
                    aria-label={user.inlinePolicies.length > 0 ? 'Edit inline policy' : 'Add inline policy'}
                    onclick={() => openPolicyEditorForUser(user)}
                  >
                    <FileJson class="size-4" />
                  </button>
                  <button
                    type="button"
                    class="text-muted-foreground transition-colors hover:text-destructive"
                    title="Delete user"
                    aria-label="Delete user"
                    onclick={() => (userToDelete = user.username)}
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
  title="Create IAM user"
  description="A new access key is created automatically."
  loading={createUserMutation.isPending}
  onClose={() => { showCreate = false; newUsername = '' }}
>
  <form id="create-user-form" onsubmit={(e) => { e.preventDefault(); handleCreateUser() }} class="flex flex-col gap-1.5">
    <label for="newUsername" class="text-sm font-medium text-black dark:text-white">Username</label>
    <Input
      bind:ref={createUserInput}
      id="newUsername"
      type="text"
      bind:value={newUsername}
      placeholder="alice"
      class="bg-white dark:bg-base"
      disabled={createUserMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button type="button" variant="default" disabled={createUserMutation.isPending} onclick={() => { showCreate = false; newUsername = '' }}>
      Cancel
    </Button>
    <Button type="submit" form="create-user-form" variant="highlighted" disabled={createUserMutation.isPending || !newUsername.trim()}>
      {createUserMutation.isPending ? 'Creating…' : 'Create user'}
    </Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={showKeySecretDialog}
  title="New access key"
  onClose={() => { newKeySecret = null; showKeySecretDialog = false }}
>
  {#if newKeySecret}
    <Callout type="warning" class="mb-4">
      Copy the secret now — it will not be shown again.
    </Callout>
    <div class="space-y-3">
      <div class="flex items-center justify-between gap-2 rounded-sm bg-neutral-50 p-2 font-mono text-sm dark:bg-coolgray-200">
        <div class="min-w-0 truncate">
          <span class="text-muted-foreground">Access key</span>
          <div>{newKeySecret.accessKeyId}</div>
        </div>
        <Button variant="outline" size="sm" onclick={() => copyText(newKeySecret!.accessKeyId, 'Access key')}>
          <Copy class="size-3.5" />
          Copy
        </Button>
      </div>
      <div class="flex items-center justify-between gap-2 rounded-sm bg-neutral-50 p-2 font-mono text-sm dark:bg-coolgray-200">
        <div class="min-w-0 truncate">
          <span class="text-muted-foreground">Secret key</span>
          <div class="break-all">{newKeySecret.secretAccessKey}</div>
        </div>
        <Button variant="outline" size="sm" onclick={() => copyText(newKeySecret!.secretAccessKey, 'Secret key')}>
          <Copy class="size-3.5" />
          Copy
        </Button>
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="brand" onclick={() => { newKeySecret = null; showKeySecretDialog = false }}>Done</Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={showPolicyDialog}
  title="Inline policy"
  size="lg"
  loading={savePolicyMutation.isPending}
  onClose={() => { policyEditor = null; showPolicyDialog = false }}
>
  {#if policyEditor}
    <div class="space-y-4">
      <div class="space-y-1.5">
        <Label for="policyName">Policy name</Label>
        <Input id="policyName" bind:value={policyEditor.policyName} />
      </div>
      <div class="space-y-1.5">
        <Label for="policyDoc">Policy document (JSON)</Label>
        <textarea
          id="policyDoc"
          bind:value={policyEditor.document}
          class="input-cool min-h-64 w-full resize-y rounded-sm bg-background p-3 font-mono text-xs"
        ></textarea>
      </div>
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
          <Button variant="outline" disabled={savePolicyMutation.isPending} onclick={() => { policyEditor = null; showPolicyDialog = false }}>
            Cancel
          </Button>
          <Button variant="brand" onclick={savePolicy} disabled={savePolicyMutation.isPending}>
            {savePolicyMutation.isPending ? 'Saving…' : 'Save'}
          </Button>
        </div>
      </div>
    {/if}
  {/snippet}
</Dialog>

<ConfirmDialog
  open={userToDelete !== null}
  title="Delete user?"
  description={userToDelete ? `Remove IAM user "${userToDelete}" and all access keys.` : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deleteUserMutation.isPending}
  onConfirm={async () => {
    if (!userToDelete) return
    try {
      await deleteUserMutation.mutateAsync(userToDelete)
    } catch (err) {
      console.error('deleteUser failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to delete user')
    } finally {
      userToDelete = null
    }
  }}
  onClose={() => (userToDelete = null)}
/>

<ConfirmDialog
  open={keyToCreate !== null}
  title="Create access key?"
  description={keyToCreate ? `Create a new access key for user "${keyToCreate}". The secret will only be shown once.` : ''}
  confirmLabel="Create key"
  loading={createKeyMutation.isPending}
  onConfirm={async () => {
    if (!keyToCreate) return
    const username = keyToCreate
    try {
      await createKeyMutation.mutateAsync({ username })
    } catch (err) {
      console.error('createUserKey failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to create access key')
    } finally {
      keyToCreate = null
    }
  }}
  onClose={() => (keyToCreate = null)}
/>

<ConfirmDialog
  open={keyToDelete !== null}
  title="Delete access key?"
  description={keyToDelete ? `Revoke key ${keyToDelete.accessKeyId}?` : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deleteKeyMutation.isPending}
  onConfirm={async () => {
    if (!keyToDelete) return
    try {
      await deleteKeyMutation.mutateAsync(keyToDelete)
    } catch (err) {
      console.error('deleteUserKey failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to delete key')
    } finally {
      keyToDelete = null
    }
  }}
  onClose={() => (keyToDelete = null)}
/>

<ConfirmDialog
  open={policyToDelete !== null}
  title="Delete inline policy?"
  description={policyToDelete ? `Remove policy "${policyToDelete.policyName}"?` : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
  loading={deletePolicyMutation.isPending}
  onConfirm={async () => {
    if (!policyToDelete) return
    try {
      await deletePolicyMutation.mutateAsync(policyToDelete)
      policyEditor = null
      showPolicyDialog = false
    } catch (err) {
      console.error('deleteUserPolicy failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to delete policy')
    } finally {
      policyToDelete = null
    }
  }}
  onClose={() => (policyToDelete = null)}
/>
