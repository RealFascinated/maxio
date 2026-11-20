export interface FuzzyMatch {
  score: number
  /** Indices of matched characters in the text, for highlighting. */
  indices: number[]
}

const isBoundary = (text: string, index: number) =>
  index === 0 || /[\s/_.:-]/.test(text[index - 1]!)

/**
 * Subsequence match: every query character must appear in order.
 * Rewards prefix matches, word starts and consecutive runs; penalises gaps and length.
 */
export function fuzzyMatch(query: string, text: string): FuzzyMatch | null {
  const needle = query.trim().toLowerCase()
  if (!needle) return { score: 0, indices: [] }
  const haystack = text.toLowerCase()

  const indices: number[] = []
  let score = 0
  let from = 0
  let previous = -2
  for (const char of needle) {
    const index = haystack.indexOf(char, from)
    if (index === -1) return null
    if (index === previous + 1) score += 8
    if (isBoundary(haystack, index)) score += 10
    if (index === 0) score += 6
    score -= Math.min(index - from, 6)
    indices.push(index)
    previous = index
    from = index + 1
  }
  if (haystack.startsWith(needle)) score += 20
  else if (haystack.includes(needle)) score += 10
  return { score: score - text.length * 0.05, indices }
}
