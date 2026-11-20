<script lang="ts">
  import { cn } from '$lib/utils'

  interface Props {
    /** `null` marks a sample with no data; it breaks the line instead of plotting zero. */
    values: (number | null)[]
    color?: string
    class?: string
  }

  let { values, color = 'var(--chart-1)', class: className }: Props = $props()

  const width = 100
  const height = 28

  const paths = $derived.by(() => {
    const present = values.filter((value): value is number => value !== null)
    if (present.length < 2) return null
    const min = Math.min(...present)
    const span = Math.max(...present) - min || 1
    const step = width / (values.length - 1)
    let line = ''
    let area = ''
    let start = -1
    const close = (end: number) => {
      if (start >= 0 && end > start) area += `L${(end * step).toFixed(2)} ${height}L${(start * step).toFixed(2)} ${height}Z`
      start = -1
    }
    values.forEach((value, index) => {
      if (value === null) return close(index - 1)
      const x = (index * step).toFixed(2)
      const y = (height - 2 - ((value - min) / span) * (height - 4)).toFixed(2)
      const segment = `${start < 0 ? 'M' : 'L'}${x} ${y}`
      if (start < 0) start = index
      line += segment
      area += segment
    })
    close(values.length - 1)
    return { line, area }
  })
</script>

<svg
  viewBox="0 0 {width} {height}"
  preserveAspectRatio="none"
  class={cn('h-7 w-full overflow-visible', className)}
  aria-hidden="true"
>
  {#if paths}
    <path d={paths.area} fill={color} fill-opacity="0.12" />
    <path d={paths.line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" />
  {/if}
</svg>
