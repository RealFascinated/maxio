/** Cross-chart hover state: hovering one chart draws a crosshair at the same time in the others. */
export const chartHover = $state<{ time: number | null }>({ time: null })
