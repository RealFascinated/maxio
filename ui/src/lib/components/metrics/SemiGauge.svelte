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

  const size = 155
  const stroke = 9
  const radius = (size - stroke) / 2
  const cx = size / 2
  // Anchor center so the apex sits near the top of the viewBox.
  const cy = radius + stroke

  const startAngle = 195
  const endAngle = -15

  function polar(angleDeg: number) {
    const rad = (angleDeg * Math.PI) / 180
    return {
      x: cx + radius * Math.cos(rad),
      y: cy + radius * Math.sin(rad),
    }
  }

  function clockwiseSpan(from: number, to: number): number {
    const start = ((from % 360) + 360) % 360
    const end = ((to % 360) + 360) % 360
    return (end - start + 360) % 360
  }

  const spanDeg = clockwiseSpan(startAngle, endAngle)

  function angleAt(t: number): number {
    return startAngle + t * spanDeg
  }

  function arcBetween(a0: number, a1: number): string {
    const start = polar(a0)
    const end = polar(a1)
    const span = clockwiseSpan(a0, a1)
    const large = span > 180 ? 1 : 0
    return `M ${start.x} ${start.y} A ${radius} ${radius} 0 ${large} 1 ${end.x} ${end.y}`
  }

  const CAP_FILL: Record<string, string> = {
    'stroke-coollabs': '#6b16ed',
    'stroke-pink-500': '#ec4899',
    'stroke-sky-400': '#38bdf8',
  }

  const track = arcBetween(angleAt(0), angleAt(1))

  const pad = stroke / 2 + 4
  const peakY = cy - radius
  const endpointY = polar(startAngle).y
  const viewMinY = peakY - pad
  const viewMaxY = endpointY + pad
  const viewMinX = cx - radius - pad
  const viewWidth = (radius + pad) * 2
  const viewHeight = viewMaxY - viewMinY
  const labelOverlap = Math.round(radius * 0.42)

  let segmentArcs = $derived.by(() => {
    if (total <= 0) return [] as { d: string; class: string }[]
    const arcs: { d: string; class: string }[] = []
    let t = 0
    for (const segment of segments) {
      if (segment.value <= 0) continue
      const fraction = segment.value / total
      arcs.push({
        d: arcBetween(angleAt(t), angleAt(t + fraction)),
        class: segment.class,
      })
      t += fraction
    }
    return arcs
  })

  const fillTotal = $derived(
    segmentArcs.length > 0
      ? segments.reduce((sum, s) => sum + (s.value > 0 ? s.value / total : 0), 0)
      : 0,
  )
  const fillsPathEnd = $derived(fillTotal >= 1 - 1e-6)
  const startCap = $derived(polar(angleAt(0)))
  const endCap = $derived(polar(angleAt(fillTotal)))
</script>

<div class={cn('flex flex-col items-center', className)}>
  <svg
    width={size}
    height={viewHeight}
    viewBox={`${viewMinX} ${viewMinY} ${viewWidth} ${viewHeight}`}
    role="img"
    aria-label={centerLabel ? `${centerLabel}: ${centerValue}` : centerValue}
  >
    <path
      d={track}
      fill="none"
      class="stroke-neutral-200 dark:stroke-coolgray-300"
      stroke-width={stroke}
      stroke-linecap="round"
    />
    {#each segmentArcs as seg, i (i)}
      <path
        d={seg.d}
        fill="none"
        class={seg.class}
        stroke-width={stroke}
        stroke-linecap="butt"
      />
    {/each}
    {#if segmentArcs.length > 0}
      <circle
        cx={startCap.x}
        cy={startCap.y}
        r={stroke / 2}
        fill={CAP_FILL[segmentArcs[0].class] ?? '#6b16ed'}
      />
      {#if fillsPathEnd}
        <circle
          cx={endCap.x}
          cy={endCap.y}
          r={stroke / 2}
          fill={CAP_FILL[segmentArcs[segmentArcs.length - 1].class] ?? '#6b16ed'}
        />
      {/if}
    {/if}
  </svg>
  <div class="text-center" style:margin-top={`-${labelOverlap}px`}>
    {#if centerLabel}
      <p class="text-xs font-medium uppercase tracking-wide text-neutral-500">{centerLabel}</p>
    {/if}
    <p class="text-2xl font-bold tabular-nums dark:text-white">{centerValue}</p>
    {#if centerUnit}
      <p class="text-xs font-medium text-neutral-500">{centerUnit}</p>
    {/if}
  </div>
</div>
