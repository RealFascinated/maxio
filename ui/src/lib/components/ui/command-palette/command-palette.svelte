<script lang="ts" module>
  import type { LucideIcon } from '$lib/lucide'

  export interface CommandItem {
    id: string
    label: string
    group: string
    icon?: LucideIcon
    /** Extra search terms that are not shown. */
    keywords?: string
    /** Right-aligned hint, e.g. 'g b'. */
    shortcut?: string
    onSelect: () => void
  }
</script>

<script lang="ts">
  import { tick } from 'svelte'
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import CornerDownLeft from 'lucide-svelte/icons/corner-down-left'
  import Search from 'lucide-svelte/icons/search'
  import { Kbd } from '$lib/components/ui/kbd'
  import { fuzzyMatch } from '$lib/fuzzy'
  import { comboKeys } from '$lib/shortcuts'
  import { cn } from '$lib/utils'

  type Props = {
    open?: boolean
    items: CommandItem[]
    placeholder?: string
    /** localStorage key for recently used item ids. */
    recentKey?: string
  }

  let {
    open = $bindable(false),
    items,
    placeholder = 'Search or jump to…',
    recentKey = 'command-palette:recent',
  }: Props = $props()

  const RECENT_LIMIT = 5

  let query = $state('')
  let activeIndex = $state(0)
  let listElement = $state<HTMLElement | null>(null)
  let recentIds = $state<string[]>([])

  interface Row {
    item: CommandItem
    indices: number[]
    heading?: string
  }

  const rows = $derived.by<Row[]>(() => {
    const trimmed = query.trim()
    if (trimmed) {
      const scored: (Row & { score: number })[] = []
      for (const item of items) {
        const byLabel = fuzzyMatch(trimmed, item.label)
        const byKeywords = byLabel || !item.keywords ? null : fuzzyMatch(trimmed, item.keywords)
        if (byLabel) scored.push({ item, indices: byLabel.indices, score: byLabel.score })
        else if (byKeywords) scored.push({ item, indices: [], score: byKeywords.score - 15 })
      }
      return scored.sort((a, b) => b.score - a.score).slice(0, 50)
    }
    const recent = recentIds
      .map((id) => items.find((item) => item.id === id))
      .filter((item) => item !== undefined)
    const recentSet = new Set(recent.map((item) => item.id))
    const ordered = [
      ...recent.map((item) => ({ item, heading: 'Recent' })),
      ...items.filter((item) => !recentSet.has(item.id)).map((item) => ({ item, heading: item.group })),
    ]
    return ordered.map((entry, index) => ({
      item: entry.item,
      indices: [],
      heading: ordered[index - 1]?.heading === entry.heading ? undefined : entry.heading,
    }))
  })

  $effect(() => {
    if (!open) return
    query = ''
    activeIndex = 0
    try {
      recentIds = JSON.parse(localStorage.getItem(recentKey) ?? '[]')
    } catch {
      recentIds = []
    }
  })

  $effect(() => {
    query
    activeIndex = 0
  })

  function select(row: Row | undefined) {
    if (!row) return
    recentIds = [row.item.id, ...recentIds.filter((id) => id !== row.item.id)].slice(0, RECENT_LIMIT)
    localStorage.setItem(recentKey, JSON.stringify(recentIds))
    open = false
    row.item.onSelect()
  }

  async function move(delta: number) {
    if (rows.length === 0) return
    activeIndex = (activeIndex + delta + rows.length) % rows.length
    await tick()
    listElement?.querySelector(`[data-index="${activeIndex}"]`)?.scrollIntoView({ block: 'nearest' })
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || (event.ctrlKey && event.key === 'n')) {
      event.preventDefault()
      move(1)
    } else if (event.key === 'ArrowUp' || (event.ctrlKey && event.key === 'p')) {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      select(rows[activeIndex])
    }
  }

  function highlight(label: string, indices: number[]): { text: string; match: boolean }[] {
    if (indices.length === 0) return [{ text: label, match: false }]
    const hit = new Set(indices)
    const parts: { text: string; match: boolean }[] = []
    for (let index = 0; index < label.length; index++) {
      const match = hit.has(index)
      const last = parts[parts.length - 1]
      if (last && last.match === match) last.text += label[index]
      else parts.push({ text: label[index]!, match })
    }
    return parts
  }
</script>

<DialogPrimitive.Root bind:open>
  <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay
      class="fixed inset-0 z-50 bg-black/50 data-[state=open]:animate-[ui-fade-in_120ms_ease-out]"
    />
    <DialogPrimitive.Content
      aria-label="Command palette"
      class="fixed left-1/2 top-[15vh] z-50 w-[calc(100vw-2rem)] max-w-xl -translate-x-1/2 overflow-hidden rounded-surface border border-border-strong bg-raised shadow-pop outline-none data-[state=open]:animate-[ui-pop-in_120ms_ease-out]"
    >
      <DialogPrimitive.Title class="sr-only">Command palette</DialogPrimitive.Title>
      <div class="flex h-10 items-center gap-2 border-b border-border px-3">
        <Search class="size-4 shrink-0 text-subtle" aria-hidden="true" />
        <!-- svelte-ignore a11y_autofocus -->
        <input
          bind:value={query}
          onkeydown={handleKeydown}
          {placeholder}
          autofocus
          autocomplete="off"
          spellcheck="false"
          role="combobox"
          aria-expanded="true"
          aria-controls="command-palette-list"
          aria-activedescendant={rows[activeIndex] ? `command-item-${activeIndex}` : undefined}
          class="h-full min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-subtle focus-visible:outline-none"
        />
        <Kbd>Esc</Kbd>
      </div>

      <div
        bind:this={listElement}
        id="command-palette-list"
        role="listbox"
        class="max-h-80 overflow-y-auto p-1"
      >
        {#each rows as row, index (row.item.id)}
          {@const Icon = row.item.icon}
          {#if row.heading}
            <div class="px-2 pt-2 pb-1 text-xs font-medium text-subtle first:pt-1">{row.heading}</div>
          {/if}
          <div
            id="command-item-{index}"
            role="option"
            tabindex="-1"
            aria-selected={index === activeIndex}
            data-index={index}
            class={cn(
              'flex h-8 cursor-pointer items-center gap-2 rounded-control px-2 text-sm',
              index === activeIndex && 'bg-hover',
            )}
            onpointermove={() => (activeIndex = index)}
            onclick={() => select(row)}
            onkeydown={null}
          >
            {#if Icon}<Icon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />{/if}
            <span class="min-w-0 flex-1 truncate">
              {#each highlight(row.item.label, row.indices) as part}
                <span class={part.match ? 'font-semibold text-accent' : ''}>{part.text}</span>
              {/each}
            </span>
            {#if query.trim()}
              <span class="shrink-0 text-xs text-subtle">{row.item.group}</span>
            {:else if row.item.shortcut}
              <span class="flex shrink-0 gap-0.5">
                {#each comboKeys(row.item.shortcut) as key}<Kbd>{key}</Kbd>{/each}
              </span>
            {/if}
          </div>
        {:else}
          <div class="px-2 py-6 text-center text-sm text-subtle">No results</div>
        {/each}
      </div>

      <div class="flex h-7 items-center justify-end gap-3 border-t border-border bg-inset px-3 text-xs text-subtle">
        <span class="flex items-center gap-1"><Kbd>↑</Kbd><Kbd>↓</Kbd> navigate</span>
        <span class="flex items-center gap-1"><Kbd><CornerDownLeft class="size-2.5" /></Kbd> select</span>
      </div>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
