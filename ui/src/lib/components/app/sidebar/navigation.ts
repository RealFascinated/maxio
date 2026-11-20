import Database from 'lucide-svelte/icons/database'
import UsersIcon from 'lucide-svelte/icons/users'
import Shield from 'lucide-svelte/icons/shield'
import BarChart2 from 'lucide-svelte/icons/bar-chart-2'
import ClipboardList from 'lucide-svelte/icons/clipboard-list'
import Settings from 'lucide-svelte/icons/settings'
import type { LucideIcon } from '$lib/lucide'
import { isBucketsNavActive, routes } from '$lib/navigation'

export interface SidebarNavEntry {
  id: string
  label: string
  icon: LucideIcon
  href: string
  active: boolean
  /** `footer` items sit at the bottom of the rail. */
  placement: 'main' | 'footer'
}

export interface NavContext {
  pathname: string
  isRootUser: boolean
}

interface NavItemDef {
  id: string
  label: string
  icon: LucideIcon
  href: string
  placement?: 'main' | 'footer'
  visible?: (ctx: NavContext) => boolean
  isActive: (ctx: NavContext) => boolean
}

const mainNavItems: NavItemDef[] = [
  {
    id: 'buckets',
    label: 'Buckets',
    icon: Database,
    href: routes.home(),
    isActive: (ctx) => isBucketsNavActive(ctx.pathname),
  },
  {
    id: 'users',
    label: 'Users',
    icon: UsersIcon,
    href: routes.users(),
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.pathname === routes.users(),
  },
  {
    id: 'policies',
    label: 'Policies',
    icon: Shield,
    href: routes.policies(),
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.pathname === routes.policies(),
  },
  {
    id: 'metrics',
    label: 'Metrics',
    icon: BarChart2,
    href: routes.metrics(),
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.pathname === routes.metrics(),
  },
  {
    id: 'jobs',
    label: 'Jobs',
    icon: ClipboardList,
    href: routes.jobs(),
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.pathname === routes.jobs(),
  },
  {
    id: 'settings',
    label: 'Settings',
    icon: Settings,
    href: routes.serverSettings(),
    placement: 'footer',
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.pathname === routes.serverSettings(),
  },
]

export function buildSidebarNavItems(ctx: NavContext): SidebarNavEntry[] {
  return mainNavItems
    .filter((item) => item.visible?.(ctx) ?? true)
    .map((item) => ({
      id: item.id,
      label: item.label,
      icon: item.icon,
      href: item.href,
      active: item.isActive(ctx),
      placement: item.placement ?? 'main',
    }))
}
