export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  if (i === 0) return `${bytes} B`
  const value = bytes / 1024 ** i
  return `${parseFloat(value.toFixed(2))} ${units[i]}`
}
