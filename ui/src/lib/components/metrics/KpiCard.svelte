<script lang="ts">
  import { StatTile } from '$lib/components/ui/stat-tile'
  import Sparkline from './Sparkline.svelte'

  interface Props {
    label: string
    value: string
    values?: (number | null)[]
    /** Percent change from the start of the selected range. */
    delta?: number | null
    color?: string
  }

  let { label, value, values = [], delta = null, color }: Props = $props()

  const deltaText = $derived(
    delta === null
      ? undefined
      : `${delta > 0 ? '+' : ''}${Math.abs(delta) >= 100 ? Math.round(delta) : delta.toFixed(1)}%`,
  )
</script>

<StatTile {label} {value} unit={deltaText}>
  {#snippet chart()}
    <Sparkline {values} {color} />
  {/snippet}
</StatTile>
