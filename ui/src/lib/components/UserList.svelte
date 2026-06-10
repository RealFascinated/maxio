<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
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

  let showCreate = $state(false)
  let newUsername = $state('')
  let userToDelete = $state<string | null>(null)
  let keyToDelete = $state<{ username: string; accessKeyId: string } | null>(null)
  let newKeySecret = $state<{ username: string; accessKeyId: string; secretAccessKey: string } | null>(null)
  let policyEditor = $state<{ username: string; policyName: string; document: string } | null>(null)
  let policyToDelete = $state<{ username: string; policyName: string } | null>(null)
  let showKeySecretDialog = $state(false)
  let showPolicyDialog = $state(false)

  const usersQuery = createQuery(() => ({
    queryKey: userKeys.list(),
    queryFn: listUsers,
  }))

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

  async function handleCreateKey(username: string) {
    try {
      await createKeyMutation.mutateAsync({ username })
    } catch (err) {
      console.error('createUserKey failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to create access key')
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
      Failed to load users. Check the server console for details.
    </Callout>
  {:else if usersQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading users…</p>
  {:else if (usersQuery.data?.users.length ?? 0) === 0}
    <Callout>No IAM users yet. Create one to grant scoped console and S3 access.</Callout>
  {:else}
    <div class="rounded-sm border bg-card">
      <Table.Root>
        <Table.Header>
          <Table.Row>
            <Table.Head>Username</Table.Head>
            <Table.Head>User ID</Table.Head>
            <Table.Head>Access keys</Table.Head>
            <Table.Head>Policies</Table.Head>
            <Table.Head class="w-48 text-right">Actions</Table.Head>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {#each usersQuery.data?.users ?? [] as user (user.username)}
            <Table.Row>
              <Table.Cell class="font-medium">{user.username}</Table.Cell>
              <Table.Cell class="font-mono text-xs text-muted-foreground">{user.userId}</Table.Cell>
              <Table.Cell>
                <div class="flex flex-wrap gap-1">
                  {#each user.accessKeys as key (key.accessKeyId)}
                    <span class="rounded-sm bg-neutral-100 px-1.5 py-0.5 font-mono text-xs dark:bg-coolgray-200">{key.accessKeyId}</span>
                  {:else}
                    <span class="text-xs text-muted-foreground">None</span>
                  {/each}
                </div>
              </Table.Cell>
              <Table.Cell>
                <div class="flex flex-wrap gap-1">
                  {#each user.inlinePolicies as policyName}
                    <button
                      type="button"
                      class="rounded-sm bg-neutral-100 px-1.5 py-0.5 font-mono text-xs dark:bg-coolgray-200"
                      onclick={() => openPolicyEditor(user, policyName)}
                    >{policyName}</button>
                  {/each}
                  {#each user.attachedPolicies as arn}
                    <span class="rounded-sm border border-neutral-200 px-1.5 py-0.5 font-mono text-xs dark:border-coolgray-300">{arn.split('/').pop()}</span>
                  {/each}
                </div>
              </Table.Cell>
              <Table.Cell class="text-right">
                <div class="flex justify-end gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    title="Create access key"
                    aria-label="Create access key"
                    onclick={() => handleCreateKey(user.username)}
                  >
                    <KeyRound class="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    title="Edit inline policy"
                    aria-label="Edit inline policy"
                    onclick={() => openNewPolicyEditor(user)}
                  >
                    <FileJson class="size-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    title="Delete user"
                    aria-label="Delete user"
                    onclick={() => (userToDelete = user.username)}
                  >
                    <Trash2 class="size-4 text-destructive" />
                  </Button>
                </div>
              </Table.Cell>
            </Table.Row>
            {#if user.accessKeys.length > 0}
              <Table.Row>
                <Table.Cell colspan={5} class="bg-muted/30 py-2">
                  <div class="flex flex-wrap items-center gap-2 pl-2 text-xs">
                    <span class="text-muted-foreground">Keys:</span>
                    {#each user.accessKeys as key (key.accessKeyId)}
                      <span class="font-mono">{key.accessKeyId}</span>
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-1"
                        onclick={() =>
                          (keyToDelete = { username: user.username, accessKeyId: key.accessKeyId })}
                      >
                        <Trash2 class="size-3 text-destructive" />
                      </Button>
                    {/each}
                  </div>
                </Table.Cell>
              </Table.Row>
            {/if}
          {/each}
        </Table.Body>
      </Table.Root>
    </div>
  {/if}
</div>

<Dialog bind:open={showCreate} title="Create IAM user">
  <div class="space-y-4">
    <div class="space-y-1.5">
      <Label for="newUsername">Username</Label>
      <Input id="newUsername" bind:value={newUsername} placeholder="alice" />
    </div>
    <div class="flex justify-end gap-2">
      <Button variant="outline" onclick={() => (showCreate = false)}>Cancel</Button>
      <Button variant="brand" onclick={handleCreateUser} disabled={createUserMutation.isPending}>
        Create
      </Button>
    </div>
  </div>
</Dialog>

<Dialog bind:open={showKeySecretDialog} title="New access key">
  {#if newKeySecret}
    <Callout type="warning" class="mb-4">
      Copy the secret now — it will not be shown again.
    </Callout>
    <div class="space-y-3 font-mono text-sm">
      <div><span class="text-muted-foreground">Access key:</span> {newKeySecret.accessKeyId}</div>
      <div><span class="text-muted-foreground">Secret key:</span> {newKeySecret.secretAccessKey}</div>
    </div>
    <div class="mt-4 flex justify-end">
      <Button variant="brand" onclick={() => { newKeySecret = null; showKeySecretDialog = false }}>Done</Button>
    </div>
  {/if}
</Dialog>

<Dialog bind:open={showPolicyDialog} title="Inline policy">
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
          class="input-cool min-h-48 w-full resize-y rounded-sm bg-background p-3 font-mono text-xs"
        ></textarea>
      </div>
      <div class="flex justify-between gap-2">
        {#if policyEditor.policyName}
          <Button
            variant="destructive"
            onclick={() =>
              (policyToDelete = {
                username: policyEditor!.username,
                policyName: policyEditor!.policyName,
              })}
          >
            Delete policy
          </Button>
        {/if}
        <div class="ml-auto flex gap-2">
          <Button variant="outline" onclick={() => { policyEditor = null; showPolicyDialog = false }}>Cancel</Button>
          <Button variant="brand" onclick={savePolicy} disabled={savePolicyMutation.isPending}>
            Save
          </Button>
        </div>
      </div>
    </div>
  {/if}
</Dialog>

<ConfirmDialog
  open={userToDelete !== null}
  title="Delete user?"
  description={userToDelete ? `Remove IAM user "${userToDelete}" and all access keys.` : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
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
  open={keyToDelete !== null}
  title="Delete access key?"
  description={keyToDelete ? `Revoke key ${keyToDelete.accessKeyId}?` : ''}
  confirmLabel="Delete"
  confirmVariant="destructive"
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
  onConfirm={async () => {
    if (!policyToDelete) return
    try {
      await deletePolicyMutation.mutateAsync(policyToDelete)
      policyEditor = null
    } catch (err) {
      console.error('deleteUserPolicy failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to delete policy')
    } finally {
      policyToDelete = null
    }
  }}
  onClose={() => (policyToDelete = null)}
/>
