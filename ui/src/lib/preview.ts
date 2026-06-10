export type PreviewKind = 'image' | 'pdf' | 'text' | 'unsupported'

export const TEXT_PREVIEW_CAP = 1024 * 1024 // 1 MiB
export const BINARY_PREVIEW_CAP = 10 * 1024 * 1024 // 10 MiB

// application/* subtypes that are really text (some clients label scripts and
// config files with these rather than a text/* type).
const TEXT_APP_TYPES = new Set([
  'application/json',
  'application/xml',
  'application/javascript',
  'application/x-javascript',
  'application/x-sh',
  'application/x-shellscript',
  'application/yaml',
  'application/x-yaml',
  'application/toml',
  'application/x-toml',
])

function normalize(contentType: string): string {
  return contentType.split(';')[0].trim().toLowerCase()
}

function isJsonFilename(filename: string): boolean {
  const lower = filename.toLowerCase()
  return lower.endsWith('.json') || lower.endsWith('.jsonc')
}

export function previewKind(contentType: string, filename = ''): PreviewKind {
  const ct = normalize(contentType)
  if (!ct && filename && isJsonFilename(filename)) return 'text'

  // SVG renders safely via <img> (no script execution in that context).
  if (ct.startsWith('image/')) return 'image'
  if (ct === 'application/pdf') return 'pdf'

  // text/* is previewable EXCEPT text/html, which we never render inline to
  // avoid stored-XSS in the console's same-origin session.
  if (ct === 'text/html') return 'unsupported'
  if (ct.startsWith('text/')) return 'text'

  if (TEXT_APP_TYPES.has(ct)) return 'text'
  if (ct.endsWith('+json') || ct.endsWith('+xml')) return 'text'

  if (filename && isJsonFilename(filename)) return 'text'

  return 'unsupported'
}

export function isPreviewable(contentType: string, filename = ''): boolean {
  return previewKind(contentType, filename) !== 'unsupported'
}

/** Normalize API or file JSON to a string for editing or display. */
export function jsonDocumentText(document: unknown): string {
  if (typeof document === 'string') return document
  if (document !== null && typeof document === 'object') {
    return JSON.stringify(document)
  }
  return String(document ?? '')
}

/** Pretty-print JSON when the value parses; otherwise return null. */
export function tryPrettyJson(text: unknown): string | null {
  if (text !== null && typeof text === 'object') {
    try {
      return JSON.stringify(text, null, 2)
    } catch {
      return null
    }
  }
  if (typeof text !== 'string') return null
  const trimmed = text.trim()
  if (!trimmed) return null
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2)
  } catch {
    return null
  }
}
