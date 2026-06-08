export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  if (i === 0) return `${bytes} B`
  const value = bytes / 1024 ** i
  return `${parseFloat(value.toFixed(2))} ${units[i]}`
}

export function formatThroughput(bytesPerSec: number): { value: string; unit: string } {
  if (bytesPerSec <= 0) return { value: '0', unit: 'Bytes/s' }
  const units = ['Bytes/s', 'KB/s', 'MB/s', 'GB/s', 'TB/s']
  const i = Math.min(Math.floor(Math.log(bytesPerSec) / Math.log(1024)), units.length - 1)
  if (i === 0) return { value: Math.round(bytesPerSec).toLocaleString(), unit: units[i] }
  const value = bytesPerSec / 1024 ** i
  return { value: parseFloat(value.toFixed(2)).toLocaleString(), unit: units[i] }
}
