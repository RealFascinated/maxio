<script lang="ts">
  import { cn } from '$lib/utils.js'

  export interface GaugeSegment {
    value: number
    class: string
  }

  interface Props {
    segments: GaugeSegment[]
    total: number
    centerLabel?: string
    centerValue: string
    centerUnit?: string
    class?: string
  }

  let {
    segments,
    total,
    centerLabel,
    centerValue,
    centerUnit,
    class: className,
  }: Props = $props()

  const size = 160
  const stroke = 10
  const radius = (size - stroke) / 2
  const cx = size / 2
  const cy = size / 2 + 8

  function arcPath(startAngle: number, endAngle: number): string {
    const start = polar(startAngle)
    const end = polar(endAngle)
    const large = endAngle - startAngle > 180 ? 1 : 0
    return `M ${start.x} ${start.y} A ${radius} ${radius} 0 ${large} 1 ${end.x} ${end.y}`
  }

  function polar(angleDeg: number) {
    const rad = (angleDeg * Math.PI) / 180
    return {
      x: cx + radius * Math.cos(rad),
      y: cy + radius * Math.sin(rad),
    }
  }

  const startAngle = 200
  const endAngle = -20
  const span = startAngle - endAngle

  let segmentArcs = $derived.by(() => {
    if (total <= 0) return [] as { d: string; class: string }[]
    let cursor = startAngle
    const arcs: { d: string; class: string }[] = []
    for (const segment of segments) {
      if (segment.value <= 0) continue
      const slice = (segment.value / total) * span
      const segEnd = cursor - slice
      arcs.push({ d: arcPath(cursor, segEnd), class: segment.class })
      cursor = segEnd
    }
    return arcs
  })
</script>

<div class={cn('flex flex-col items-center', className)}>
  <svg
    width={size}
    height={size * 0.62}
    viewBox={`0 0 ${size} ${size * 0.62}`}
    role="img"
    aria-label={centerLabel ? `${centerLabel}: ${centerValue}` : centerValue}
  >
    <path
      d={arcPath(startAngle, endAngle)}
      fill="none"
      class="stroke-neutral-200 dark:stroke-coolgray-300"
      stroke-width={stroke}
      stroke-linecap="round"
    />
    {#each segmentArcs as arc (arc.d)}
      <path
        d={arc.d}
        fill="none"
        class={arc.class}
        stroke-width={stroke}
        stroke-linecap="round"
      />
    {/each}
  </svg>
  <div class="-mt-14 text-center">
    {#if centerLabel}
      <p class="text-xs font-medium uppercase tracking-wide text-neutral-500">{centerLabel}</p>
    {/if}
    <p class="text-2xl font-bold tabular-nums dark:text-white">{centerValue}</p>
    {#if centerUnit}
      <p class="text-xs font-medium text-neutral-500">{centerUnit}</p>
    {/if}
  </div>
</div>
