<script lang="ts" module>
  export interface ChartSeries {
    key: string
    label: string
    color: string
    /** Aligned with the chart's `t`; `null` leaves a gap. */
    values: (number | null)[]
    dashed?: boolean
  }

  export interface ChartRefLine {
    value: number
    label: string
  }
</script>

<script lang="ts">
  import type { MetricsRange } from '$lib/api/metrics'
  import { cn } from '$lib/utils'
  import { chartHover } from './chart-hover.svelte'
  import {
    formatAxisTime,
    formatCompact,
    formatTooltipTime,
    lastValue,
    niceByteTicks,
    niceTicks,
    timeTicks,
  } from './chart-utils'

  interface Props {
    t: number[]
    series: ChartSeries[]
    range: MetricsRange
    /** `line` draws bare lines, `area` adds a faint fill, `stacked` stacks the series. */
    mode?: 'line' | 'area' | 'stacked'
    /** Formats values for the tooltip, legend, and accessible summary. */
    format?: (value: number) => string
    /** Formats y axis labels; defaults to `format`. */
    axisFormat?: (value: number) => string
    /** Pick 1024-based axis ticks (for byte values). */
    bytes?: boolean
    height?: number
    /** Lower bound for the top of the y axis, e.g. 100 for percentages. */
    yMax?: number
    refLine?: ChartRefLine
    label: string
    class?: string
  }

  let {
    t,
    series,
    range,
    mode = 'line',
    format = formatCompact,
    axisFormat,
    bytes = false,
    height = 200,
    yMax,
    refLine,
    label,
    class: className,
  }: Props = $props()

  let wrapper = $state<HTMLDivElement>()
  let width = $state(0)
  let hidden = $state<string[]>([])
  let localIndex = $state<number | null>(null)

  const fmtAxis = $derived(axisFormat ?? format)
  const visible = $derived(series.filter((item) => !hidden.includes(item.key)))
  /** Stacked layers as [lower, upper] pairs per series; other modes use [0, value]. */
  const layers = $derived.by(() => {
    const running = new Array<number>(t.length).fill(0)
    return visible.map((item) => ({
      item,
      bounds: item.values.map((value, i): [number, number] | null => {
        if (value === null) return null
        if (mode !== 'stacked') return [0, value]
        const lower = running[i]
        running[i] += value
        return [lower, running[i]]
      }),
    }))
  })

  const yTop = $derived.by(() => {
    let max = 0
    for (const layer of layers) for (const bound of layer.bounds) if (bound && bound[1] > max) max = bound[1]
    const floor = Math.max(yMax ?? 0, refLine?.value ?? 0)
    // Pad only when the data outgrows the fixed ceiling, so a 0-100 axis stays 0-100.
    const top = max > floor ? max * 1.05 : floor
    if (top <= 0) return { ticks: [0, 1], max: 1 }
    const ticks = bytes ? niceByteTicks(0, top) : niceTicks(0, top)
    return { ticks, max: ticks[ticks.length - 1] }
  })

  const leftMargin = $derived(
    Math.max(36, 14 + 6 * Math.max(...yTop.ticks.map((tick) => fmtAxis(tick).length))),
  )

  const margin = $derived({ top: 8, right: 16, bottom: 22, left: leftMargin })

  const innerWidth = $derived(Math.max(width - margin.left - margin.right, 1))
  const innerHeight = $derived(height - margin.top - margin.bottom)
  const t0 = $derived(t[0] ?? 0)
  const t1 = $derived(t[t.length - 1] ?? 1)

  const x = (time: number) =>
    margin.left + (t1 === t0 ? 0 : ((time - t0) / (t1 - t0)) * innerWidth)

  const y = (value: number) => margin.top + innerHeight - (value / yTop.max) * innerHeight

  const xTicks = $derived(timeTicks(t0, t1, Math.max(2, Math.floor(innerWidth / 80))))

  /** Contiguous index ranges where `present(i)` holds. */
  function runs(present: (index: number) => boolean): [number, number][] {
    const result: [number, number][] = []
    let start = -1
    for (let i = 0; i <= t.length; i++) {
      if (i < t.length && present(i)) {
        if (start < 0) start = i
      } else if (start >= 0) {
        result.push([start, i - 1])
        start = -1
      }
    }
    return result
  }

  const paths = $derived(
    layers.map(({ item, bounds }) => {
      const pieces = runs((i) => bounds[i] !== null)
      let line = ''
      let area = ''
      for (const [start, end] of pieces) {
        if (start === end) {
          line += `M${x(t[start]).toFixed(1)} ${y(bounds[start]![1]).toFixed(1)}h.01`
          continue
        }
        let top = ''
        let bottom = ''
        for (let i = start; i <= end; i++) {
          top += `${i === start ? 'M' : 'L'}${x(t[i]).toFixed(1)} ${y(bounds[i]![1]).toFixed(1)}`
        }
        line += top
        if (mode === 'line') continue
        for (let i = end; i >= start; i--) {
          bottom += `L${x(t[i]).toFixed(1)} ${y(bounds[i]![0]).toFixed(1)}`
        }
        area += `${top}${bottom}Z`
      }
      return { item, line, area }
    }),
  )

  function nearestIndex(time: number | null): number | null {
    if (time === null || t.length === 0 || time < t0 || time > t1) return null
    let low = 0
    let high = t.length - 1
    while (high - low > 1) {
      const mid = (low + high) >> 1
      if (t[mid] <= time) low = mid
      else high = mid
    }
    return time - t[low] <= t[high] - time ? low : high
  }

  const activeIndex = $derived(localIndex ?? nearestIndex(chartHover.time))

  function setIndex(index: number | null) {
    localIndex = index
    chartHover.time = index === null ? null : t[index]
  }

  function onPointerMove(event: PointerEvent) {
    if (!wrapper) return
    const px = event.clientX - wrapper.getBoundingClientRect().left
    const ratio = Math.min(1, Math.max(0, (px - margin.left) / innerWidth))
    setIndex(nearestIndex(t0 + ratio * (t1 - t0)))
  }

  function onKeyDown(event: KeyboardEvent) {
    const last = t.length - 1
    const current = localIndex ?? last
    const next =
      event.key === 'ArrowLeft' ? current - 1
      : event.key === 'ArrowRight' ? current + 1
      : event.key === 'Home' ? 0
      : event.key === 'End' ? last
      : null
    if (event.key === 'Escape') setIndex(null)
    if (next === null) return
    event.preventDefault()
    setIndex(Math.min(last, Math.max(0, next)))
  }

  function toggle(key: string) {
    hidden = hidden.includes(key) ? hidden.filter((entry) => entry !== key) : [...hidden, key]
  }

  const legendIndex = $derived(activeIndex ?? t.length - 1)
  const tooltipTotal = $derived(
    mode === 'stacked' && localIndex !== null
      ? visible.reduce((sum, item) => sum + (item.values[localIndex!] ?? 0), 0)
      : null,
  )

  const summary = $derived(
    `${label}. ${series
      .map((item) => {
        const latest = lastValue(item.values)
        return `${item.label}: ${latest === null ? 'no data' : `latest ${format(latest)}`}`
      })
      .join('; ')}.`,
  )
</script>

<div class={cn('w-full', className)}>
  <div bind:this={wrapper} bind:clientWidth={width} class="relative w-full" style="height: {height}px">
    {#if width > 0}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <svg
        {width}
        {height}
        viewBox="0 0 {width} {height}"
        role="img"
        aria-label={summary}
        tabindex="0"
        class="block touch-pan-y rounded-control outline-none"
        onkeydown={onKeyDown}
        onblur={() => setIndex(null)}
      >
        {#each yTop.ticks as tick (tick)}
          <line
            x1={margin.left}
            x2={width - margin.right}
            y1={y(tick)}
            y2={y(tick)}
            class="stroke-border"
            stroke-dasharray={tick === 0 ? undefined : '2 3'}
          />
          <text
            x={margin.left - 8}
            y={y(tick)}
            dy="0.32em"
            text-anchor="end"
            font-size="10"
            class="num fill-muted-foreground"
          >
            {fmtAxis(tick)}
          </text>
        {/each}

        {#each xTicks as tick (tick)}
          {@const px = x(tick)}
          <text
            x={px}
            y={height - 6}
            text-anchor={px > width - 28 ? 'end' : 'middle'}
            font-size="10"
            class="num fill-muted-foreground"
          >
            {formatAxisTime(tick, range)}
          </text>
        {/each}

        {#if refLine}
          <line
            x1={margin.left}
            x2={width - margin.right}
            y1={y(refLine.value)}
            y2={y(refLine.value)}
            stroke="var(--danger)"
            stroke-dasharray="4 3"
            stroke-opacity="0.8"
          />
          <text x={width - margin.right - 4} y={y(refLine.value) - 4} text-anchor="end" font-size="10" fill="var(--danger)">
            {refLine.label}
          </text>
        {/if}

        {#each paths as path (path.item.key)}
          {#if path.area}
            <path d={path.area} fill={path.item.color} fill-opacity={mode === 'stacked' ? 0.55 : 0.14} />
          {/if}
          <path
            d={path.line}
            fill="none"
            stroke={path.item.color}
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-dasharray={path.item.dashed ? '4 3' : undefined}
          />
        {/each}

        {#if activeIndex !== null}
          <line
            x1={x(t[activeIndex])}
            x2={x(t[activeIndex])}
            y1={margin.top}
            y2={margin.top + innerHeight}
            class="stroke-muted-foreground"
            stroke-dasharray="3 3"
          />
          {#each layers as layer (layer.item.key)}
            {@const bound = layer.bounds[activeIndex]}
            {#if bound}
              <circle
                cx={x(t[activeIndex])}
                cy={y(bound[1])}
                r="3.5"
                fill={layer.item.color}
                class="stroke-panel"
                stroke-width="1.5"
              />
            {/if}
          {/each}
        {/if}

        <rect
          x={margin.left}
          y={margin.top}
          width={innerWidth}
          height={innerHeight}
          fill="transparent"
          role="presentation"
          onpointermove={onPointerMove}
          onpointerleave={() => setIndex(null)}
        />
      </svg>

      {#if localIndex !== null}
        {@const px = x(t[localIndex])}
        <div
          class="pointer-events-none absolute top-2 z-10 min-w-40 max-w-64 rounded-surface border border-border-strong bg-raised px-2.5 py-2 text-xs text-foreground shadow-pop"
          style={px > width / 2 ? `right: ${width - px + 12}px` : `left: ${px + 12}px`}
        >
          <p class="num mb-1.5 font-medium text-foreground">
            {formatTooltipTime(t[localIndex])}
          </p>
          <ul class="space-y-1">
            {#each visible as item (item.key)}
              {@const value = item.values[localIndex]}
              <li class="flex items-center gap-2">
                <span class="size-2 shrink-0 rounded-full" style="background: {item.color}"></span>
                <span class="truncate text-muted-foreground">{item.label}</span>
                <span class="num ml-auto pl-3 font-medium text-foreground">
                  {value === null ? '—' : format(value)}
                </span>
              </li>
            {/each}
          </ul>
          {#if tooltipTotal !== null}
            <p
              class="mt-1.5 flex justify-between border-t border-border pt-1.5 font-medium text-foreground"
            >
              <span>Total</span>
              <span class="num">{format(tooltipTotal)}</span>
            </p>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  {#if series.length > 1}
    <ul class="mt-2 flex flex-wrap gap-x-1 gap-y-1">
      {#each series as item (item.key)}
        {@const value = item.values[legendIndex] ?? null}
        <li>
          <button
            type="button"
            class={cn(
              'inline-flex h-6 items-center gap-1.5 rounded-control px-1.5 text-xs transition-colors hover:bg-hover',
              hidden.includes(item.key) && 'opacity-40',
            )}
            aria-pressed={!hidden.includes(item.key)}
            aria-label="Toggle {item.label}"
            onclick={() => toggle(item.key)}
          >
            <span class="size-2 rounded-full" style="background: {item.color}"></span>
            <span class={cn('text-muted-foreground', hidden.includes(item.key) && 'line-through')}>
              {item.label}
            </span>
            {#if value !== null && !hidden.includes(item.key)}
              <span class="num font-medium text-foreground">{format(value)}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
