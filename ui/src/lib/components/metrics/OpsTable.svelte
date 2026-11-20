<script lang="ts">
  import type { OperationSnapshot } from '$lib/api/metrics'
  import { formatOperationName } from '$lib/format'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import * as Table from '$lib/components/ui/table'
  import ArrowDown from 'lucide-svelte/icons/arrow-down'
  import ArrowUp from 'lucide-svelte/icons/arrow-up'
  import { cn } from '$lib/utils'
  import { formatSeconds } from './chart-utils'

  interface Props {
    title: string
    ops: OperationSnapshot[]
    filter: string
    /** Operations whose p99 exceeds this are marked slow (seconds). */
    slowSeconds?: number
  }

  let { title, ops, filter, slowSeconds = 0.25 }: Props = $props()

  type SortKey = 'operation' | 'count' | 'avg' | 'p50' | 'p95' | 'p99'
  let sortKey = $state<SortKey>('count')
  let sortDesc = $state(true)

  const avg = (op: OperationSnapshot) => (op.count > 0 ? op.sumSeconds / op.count : 0)
  const sortValue = (op: OperationSnapshot, key: SortKey) =>
    key === 'operation' ? op.operation
    : key === 'count' ? op.count
    : key === 'avg' ? avg(op)
    : (op[`${key}Seconds` as 'p50Seconds'] ?? -1)

  const total = $derived(ops.reduce((sum, op) => sum + op.count, 0))
  const rows = $derived(
    ops
      .filter((op) => formatOperationName(op.operation).toLowerCase().includes(filter.trim().toLowerCase()))
      .toSorted((a, b) => {
        const left = sortValue(a, sortKey)
        const right = sortValue(b, sortKey)
        const order = left < right ? -1 : left > right ? 1 : 0
        return sortDesc ? -order : order
      }),
  )

  function sortBy(key: SortKey) {
    sortDesc = sortKey === key ? !sortDesc : key !== 'operation'
    sortKey = key
  }

  const columns: { key: SortKey; label: string; numeric: boolean }[] = [
    { key: 'operation', label: 'Operation', numeric: false },
    { key: 'count', label: 'Count', numeric: true },
    { key: 'avg', label: 'Avg', numeric: true },
    { key: 'p50', label: 'p50', numeric: true },
    { key: 'p95', label: 'p95', numeric: true },
    { key: 'p99', label: 'p99', numeric: true },
  ]

  const latency = (seconds: number | null) => (seconds === null ? '—' : formatSeconds(seconds))
</script>

<section class="flex flex-col gap-2">
  <h2 class="text-sm">{title}</h2>
  {#if ops.length === 0}
    <div class="rounded-surface border border-border bg-panel">
      <EmptyState title="No operations recorded" />
    </div>
  {:else}
    <Table.Root class="min-w-[36rem]">
      <Table.Header>
        <Table.Row class="hover:bg-transparent">
          {#each columns as column (column.key)}
            {@const active = sortKey === column.key}
            <Table.Head
              class={column.numeric ? 'text-right' : undefined}
              aria-sort={active ? (sortDesc ? 'descending' : 'ascending') : 'none'}
            >
              <button
                type="button"
                class={cn('inline-flex items-center gap-1 rounded-control hover:text-foreground', active && 'text-foreground')}
                onclick={() => sortBy(column.key)}
              >
                {column.label}
                {#if active}
                  {#if sortDesc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}
                {/if}
              </button>
            </Table.Head>
          {/each}
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each rows as op (op.operation)}
          {@const slow = (op.p99Seconds ?? 0) > slowSeconds}
          <Table.Row>
            <Table.Cell class="font-medium">{formatOperationName(op.operation)}</Table.Cell>
            <Table.Cell>
              <div class="flex items-center justify-end gap-2">
                <ProgressBar class="hidden w-16 sm:block" value={total > 0 ? (op.count / total) * 100 : 0} />
                <span class="num w-12 text-right text-xs text-muted-foreground">
                  {total > 0 ? ((op.count / total) * 100).toFixed(1) : '0'}%
                </span>
                <span class="num w-16 text-right">{op.count.toLocaleString()}</span>
              </div>
            </Table.Cell>
            <Table.Cell class="num text-right">{formatSeconds(avg(op))}</Table.Cell>
            <Table.Cell class="num text-right">{latency(op.p50Seconds)}</Table.Cell>
            <Table.Cell class="num text-right">{latency(op.p95Seconds)}</Table.Cell>
            <Table.Cell
              class={cn('num text-right', slow && ((op.p99Seconds ?? 0) > slowSeconds * 4 ? 'text-danger' : 'text-warning'))}
            >
              {latency(op.p99Seconds)}
            </Table.Cell>
          </Table.Row>
        {:else}
          <Table.Row class="hover:bg-transparent">
            <Table.Cell colspan={6} class="text-center text-muted-foreground">No matching operations</Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
  {/if}
</section>
