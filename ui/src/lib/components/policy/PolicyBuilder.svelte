<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { newStatement } from '$lib/policy/document'
  import type { PolicyDocument, PolicyVariant } from '$lib/policy/types'
  import PolicyStatementCard from './PolicyStatementCard.svelte'

  interface Props {
    document: PolicyDocument
    variant: PolicyVariant
    accessibleBuckets?: string[]
    contextBucket?: string
    bucketsLoading?: boolean
    readonly?: boolean
    onchange?: (document: PolicyDocument) => void
  }

  let { document = $bindable(), variant, accessibleBuckets = [], contextBucket, bucketsLoading = false, readonly = false, onchange }: Props = $props()

  function emit() {
    onchange?.(document)
  }

  function addStatement() {
    if (readonly) return
    document = {
      ...document,
      Statement: [...document.Statement, newStatement(variant, contextBucket)],
    }
    emit()
  }

  function updateStatement(index: number, stmt: (typeof document.Statement)[0]) {
    const statements = [...document.Statement]
    statements[index] = stmt
    document = { ...document, Statement: statements }
    emit()
  }

  function deleteStatement(index: number) {
    if (readonly) return
    document = {
      ...document,
      Statement: document.Statement.filter((_, i) => i !== index),
    }
    emit()
  }

  function moveStatement(index: number, direction: -1 | 1) {
    if (readonly) return
    const next = index + direction
    if (next < 0 || next >= document.Statement.length) return
    const statements = [...document.Statement]
    const tmp = statements[index]
    statements[index] = statements[next]
    statements[next] = tmp
    document = { ...document, Statement: statements }
    emit()
  }
</script>

<div class="space-y-4">
  <div class="flex flex-col gap-4">
    {#each document.Statement as stmt, i (i)}
      <PolicyStatementCard
        statement={stmt}
        index={i}
        total={document.Statement.length}
        {variant}
        {accessibleBuckets}
        {bucketsLoading}
        {readonly}
        onchange={(s) => updateStatement(i, s)}
        ondelete={() => deleteStatement(i)}
        onmove={(dir) => moveStatement(i, dir)}
      />
    {/each}
  </div>

  {#if !readonly}
    <Button type="button" variant="outline" size="sm" onclick={addStatement}>Add statement</Button>
  {/if}
</div>
