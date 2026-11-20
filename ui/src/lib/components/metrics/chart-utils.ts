import type { MetricsHistory, MetricsRange, SeriesKey } from '$lib/api/metrics'
import { formatBytes, formatThroughput } from '$lib/format-bytes'
import { formatLatency } from '$lib/format'

const chart = (slot: number) => `var(--chart-${slot})`

export const COLORS = {
  read: chart(1),
  write: chart(2),
  meta: chart(3),
  s2xx: 'var(--success)',
  s3xx: 'var(--info)',
  s4xx: 'var(--warning)',
  s5xx: 'var(--danger)',
  p50: chart(1),
  p95: chart(2),
  p99: chart(7),
  palette: [1, 2, 3, 4, 5, 6, 7, 8].map(chart),
} as const

export const RANGE_LABELS: Record<MetricsRange, string> = {
  '15m': 'Last 15 minutes',
  '1h': 'Last hour',
  '6h': 'Last 6 hours',
  '24h': 'Last 24 hours',
  '7d': 'Last 7 days',
  '30d': 'Last 30 days',
}

// ---------- value formatters (compact, for axes and tooltips) ----------

export function formatCompact(value: number): string {
  const abs = Math.abs(value)
  if (abs >= 1e9) return `${parseFloat((value / 1e9).toFixed(2))}B`
  if (abs >= 1e6) return `${parseFloat((value / 1e6).toFixed(2))}M`
  if (abs >= 1e4) return `${parseFloat((value / 1e3).toFixed(1))}k`
  if (abs >= 100) return Math.round(value).toLocaleString()
  return parseFloat(value.toFixed(2)).toLocaleString()
}

export const formatRate = (value: number) => `${formatCompact(value)}/s`
export const formatPercent = (value: number) => `${parseFloat(value.toFixed(value < 10 ? 2 : 1))}%`
export const formatRatio = (value: number) => formatPercent(value * 100)

export function formatBytesPerSec(value: number): string {
  const { value: amount, unit } = formatThroughput(value)
  return `${amount} ${unit}`
}

export function formatSeconds(value: number): string {
  if (value === 0) return '0'
  const { value: amount, unit } = formatLatency(value)
  return unit ? `${amount} ${unit}` : amount
}

export const formatByteValue = formatBytes

// ---------- axis helpers ----------

/** Evenly spaced "nice" values covering [min, max], at most roughly `count` of them. */
export function niceTicks(min: number, max: number, count = 4): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1]
  if (max <= min) max = min + 1
  const rawStep = (max - min) / count
  const magnitude = 10 ** Math.floor(Math.log10(rawStep))
  const residual = rawStep / magnitude
  const step = (residual >= 5 ? 10 : residual >= 2 ? 5 : residual >= 1.5 ? 2 : 1) * magnitude
  const ticks: number[] = []
  for (let tick = Math.floor(min / step) * step; tick < max + step * 0.999; tick += step) {
    ticks.push(parseFloat(tick.toPrecision(12)))
    if (ticks.length > 12) break
  }
  return ticks
}

/** Ticks for byte axes land on 1024-based boundaries so labels read 256 MB, 512 MB, … */
export function niceByteTicks(min: number, max: number, count = 4): number[] {
  const span = Math.max(max - min, 1)
  const unit = 1024 ** Math.max(0, Math.floor(Math.log(span) / Math.log(1024)))
  return niceTicks(min / unit, max / unit, count).map((tick) => tick * unit)
}

const TIME_STEPS_SECONDS = [
  5, 10, 15, 30, 60, 120, 300, 600, 900, 1_800, 3_600, 7_200, 10_800, 21_600, 43_200, 86_400,
  172_800, 345_600, 604_800,
]

export function timeTicks(from: number, to: number, targetCount: number): number[] {
  const spanSeconds = (to - from) / 1000
  const step =
    TIME_STEPS_SECONDS.find((candidate) => spanSeconds / candidate <= targetCount) ?? 604_800
  const stepMs = step * 1000
  const offsetMs = new Date(from).getTimezoneOffset() * -60_000
  const ticks: number[] = []
  for (let tick = Math.ceil((from + offsetMs) / stepMs) * stepMs - offsetMs; tick <= to; tick += stepMs) {
    ticks.push(tick)
  }
  return ticks
}

const pad = (value: number) => String(value).padStart(2, '0')

export function formatAxisTime(ms: number, range: MetricsRange): string {
  const date = new Date(ms)
  const clock = `${pad(date.getHours())}:${pad(date.getMinutes())}`
  if (range === '15m') return `${clock}:${pad(date.getSeconds())}`
  if (range === '7d' || range === '30d') {
    if (date.getHours() === 0 && date.getMinutes() === 0) {
      return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
    }
    return clock
  }
  return clock
}

export function formatTooltipTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

// ---------- series helpers ----------

export function seriesOf(history: MetricsHistory | undefined, key: SeriesKey): (number | null)[] {
  return history?.series[key] ?? []
}

export function lastValue(values: (number | null)[]): number | null {
  for (let i = values.length - 1; i >= 0; i--) if (values[i] !== null) return values[i]
  return null
}

export function firstValue(values: (number | null)[]): number | null {
  return values.find((value) => value !== null) ?? null
}

export function maxValue(values: (number | null)[]): number {
  let max = 0
  for (const value of values) if (value !== null && value > max) max = value
  return max
}

/** Percent change from the start of the range to its end; `null` when it cannot be computed. */
export function deltaPercent(values: (number | null)[]): number | null {
  const start = firstValue(values)
  const end = lastValue(values)
  if (start === null || end === null || start === 0) return null
  return ((end - start) / Math.abs(start)) * 100
}

/** Pointwise `numerator / denominator * 100`; gaps and empty windows stay `null`. */
export function percentOf(
  numerator: (number | null)[],
  denominator: (number | null)[],
): (number | null)[] {
  return numerator.map((value, i) => {
    const total = denominator[i]
    return value === null || total == null || total <= 0 ? null : (value / total) * 100
  })
}
