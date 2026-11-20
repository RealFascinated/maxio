<script lang="ts" module>
  export interface BarItem {
    label: string
    value: number
    href?: string
    /** Secondary text shown after the value, e.g. a share or count. */
    detail?: string
    color?: string
  }
</script>

<script lang="ts">
  interface Props {
    items: BarItem[]
    format: (value: number) => string
    color?: string
    empty?: string
  }

  let { items, format, color = 'var(--chart-1)', empty = 'No data' }: Props = $props()

  const max = $derived(Math.max(...items.map((item) => item.value), 1))
</script>

{#if items.length === 0}
  <p class="py-2 text-sm text-muted-foreground">{empty}</p>
{:else}
  <ul class="flex flex-col gap-1.5">
    {#each items as item (item.label)}
      <li class="grid grid-cols-[minmax(0,10rem)_1fr_auto] items-center gap-3 text-sm sm:grid-cols-[minmax(0,12rem)_1fr_auto]">
        {#if item.href}
          <a
            href={item.href}
            class="truncate text-foreground hover:text-accent hover:underline"
          >
            {item.label}
          </a>
        {:else}
          <span class="truncate text-foreground">{item.label}</span>
        {/if}
        <div class="h-1.5 overflow-hidden rounded-control bg-hover">
          <div
            class="h-full rounded-control"
            style="width: {(item.value / max) * 100}%; background: {item.color ?? color}"
          ></div>
        </div>
        <span class="num text-right text-xs text-foreground">
          {format(item.value)}{#if item.detail}<span class="ml-2 text-muted-foreground">{item.detail}</span>{/if}
        </span>
      </li>
    {/each}
  </ul>
{/if}
