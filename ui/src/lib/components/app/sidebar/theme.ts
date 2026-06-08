export type ThemeMode = 'light' | 'system' | 'dark'

const themeCycle: ThemeMode[] = ['light', 'system', 'dark']

export function isThemeMode(value: string | null): value is ThemeMode {
  return value === 'light' || value === 'system' || value === 'dark'
}

export function nextThemeMode(current: ThemeMode): ThemeMode {
  const index = themeCycle.indexOf(current)
  return themeCycle[(index + 1) % themeCycle.length] ?? 'system'
}

export function resolveIsDark(mode: ThemeMode): boolean {
  if (mode === 'dark') {
    return true
  }
  if (mode === 'light') {
    return false
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

export function applyThemeToDocument(mode: ThemeMode): boolean {
  const isDark = resolveIsDark(mode)
  document.documentElement.classList.toggle('dark', isDark)
  return isDark
}
