import { base } from '$lib/base'
import { routes } from '$lib/navigation'

export interface Crumb {
  label: string
  href?: string
}

const sectionLabels: Record<string, string> = {
  users: 'Users',
  policies: 'Policies',
  metrics: 'Metrics',
  jobs: 'Jobs',
  settings: 'Settings',
  design: 'Design',
}

/** Route-derived breadcrumb trail; the last crumb has no href. */
export function buildBreadcrumbs(pathname: string): Crumb[] {
  const path = pathname.startsWith(base) ? pathname.slice(base.length) : pathname
  const segments = path.split('/').filter(Boolean).map(decodeURIComponent)

  if (segments[0] === 'buckets' && segments[1]) {
    const bucket = segments[1]
    const rest = segments.slice(2)
    const crumbs: Crumb[] = [
      { label: 'Buckets', href: routes.home() },
      { label: bucket, href: rest.length ? routes.bucket(bucket) : undefined },
    ]
    if (rest.length === 1 && rest[0] === 'settings') {
      crumbs.push({ label: 'Settings' })
    } else {
      rest.forEach((segment, index) =>
        crumbs.push({
          label: segment,
          href: index < rest.length - 1 ? routes.bucket(bucket, `${rest.slice(0, index + 1).join('/')}/`) : undefined,
        }),
      )
    }
    return crumbs
  }

  return [{ label: sectionLabels[segments[0] ?? ''] ?? 'Buckets' }]
}
