/**
 * Global keyboard shortcuts.
 *
 * Combos are space-separated steps; a step is `key` or `mod+key` (mod = Ctrl or ⌘).
 *   register('mod+k', open)       single combo
 *   register(['g b', 'G'], go)    chord (press g, then b) plus an alternative
 * Plain-key shortcuts are ignored while typing in inputs or while a dialog is open;
 * `mod+` shortcuts always fire.
 */

export interface ShortcutOptions {
  description?: string
  group?: string
}

export interface ShortcutInfo {
  combo: string
  description: string
  group: string
}

interface Binding {
  sequence: string[]
  combo: string
  handler: (event: KeyboardEvent) => void
  description?: string
  group: string
}

const CHORD_TIMEOUT_MS = 1000
const bindings = new Set<Binding>()
let buffer: string[] = []
let timer: ReturnType<typeof setTimeout> | undefined
let listening = false

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)
}

function isModalOpen(): boolean {
  return document.querySelector('[role="dialog"][data-state="open"], [role="menu"][data-state="open"]') !== null
}

function tokenFor(event: KeyboardEvent): string | null {
  if (['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return null
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key
  const mod = event.ctrlKey || event.metaKey
  return `${mod ? 'mod+' : ''}${event.altKey ? 'alt+' : ''}${key}`
}

function reset() {
  buffer = []
  clearTimeout(timer)
}

function startsWith(sequence: string[], prefix: string[]): boolean {
  return prefix.every((step, index) => sequence[index] === step)
}

function handleKeydown(event: KeyboardEvent) {
  const token = tokenFor(event)
  if (!token) return
  const hasMod = token.startsWith('mod+') || token.startsWith('alt+')
  if (!hasMod && (isTyping(event.target) || isModalOpen() || event.defaultPrevented)) {
    reset()
    return
  }

  buffer.push(token)
  let candidates = [...bindings].filter((binding) => startsWith(binding.sequence, buffer))
  if (candidates.length === 0 && buffer.length > 1) {
    buffer = [token]
    candidates = [...bindings].filter((binding) => startsWith(binding.sequence, buffer))
  }
  if (candidates.length === 0) return reset()

  const exact = candidates.find((binding) => binding.sequence.length === buffer.length)
  if (exact) {
    event.preventDefault()
    reset()
    exact.handler(event)
    return
  }
  clearTimeout(timer)
  timer = setTimeout(reset, CHORD_TIMEOUT_MS)
}

/** Registers one or more combos for a handler. Returns an unregister function. */
export function register(
  combos: string | string[],
  handler: (event: KeyboardEvent) => void,
  options: ShortcutOptions = {},
): () => void {
  const added = [combos].flat().map<Binding>((combo) => ({
    combo,
    sequence: combo.split(' ').map((step) => step.toLowerCase()),
    handler,
    description: options.description,
    group: options.group ?? 'General',
  }))
  added.forEach((binding) => bindings.add(binding))
  if (!listening) {
    window.addEventListener('keydown', handleKeydown)
    listening = true
  }
  return () => {
    added.forEach((binding) => bindings.delete(binding))
    if (bindings.size === 0 && listening) {
      window.removeEventListener('keydown', handleKeydown)
      listening = false
      reset()
    }
  }
}

/** Described shortcuts, for the help dialog. First combo of each handler only. */
export function listShortcuts(): ShortcutInfo[] {
  return [...bindings]
    .filter((binding) => binding.description)
    .map((binding) => ({
      combo: binding.combo,
      description: binding.description!,
      group: binding.group,
    }))
}

const isMac = () => typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)

/** `mod+k` → ['⌘', 'K'] (or ['Ctrl', 'K']); `g b` → ['G', 'B']. */
export function comboKeys(combo: string): string[] {
  return combo.split(' ').flatMap((step) =>
    step.split('+').map((part) => {
      if (part === 'mod') return isMac() ? '⌘' : 'Ctrl'
      if (part === 'alt') return isMac() ? '⌥' : 'Alt'
      return part.length === 1 ? part.toUpperCase() : part
    }),
  )
}
