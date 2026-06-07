import Home from 'lucide-svelte/icons/home'
import UsersIcon from 'lucide-svelte/icons/users'
import type { Component } from 'svelte'

export type AppView = 'objects' | 'settings' | 'users'

export interface SidebarNavEntry {
  id: string
  label: string
  icon: Component
  active: boolean
  onSelect: () => void
}

export interface NavContext {
  currentView: AppView
  selectedBucket: string | null
  isRootUser: boolean
}

export interface NavHandlers {
  goHome: () => void
  goUsers: () => void
}

interface NavItemDef {
  id: string
  label: string
  icon: Component
  visible?: (ctx: NavContext) => boolean
  isActive: (ctx: NavContext) => boolean
  onSelect: (handlers: NavHandlers) => () => void
}

const mainNavItems: NavItemDef[] = [
  {
    id: 'buckets',
    label: 'Buckets',
    icon: Home,
    isActive: (ctx) => ctx.currentView !== 'users' && ctx.selectedBucket === null,
    onSelect: (handlers) => handlers.goHome,
  },
  {
    id: 'users',
    label: 'Users',
    icon: UsersIcon,
    visible: (ctx) => ctx.isRootUser,
    isActive: (ctx) => ctx.currentView === 'users',
    onSelect: (handlers) => handlers.goUsers,
  },
]

export function buildSidebarNavItems(
  ctx: NavContext,
  handlers: NavHandlers,
): SidebarNavEntry[] {
  return mainNavItems
    .filter((item) => item.visible?.(ctx) ?? true)
    .map((item) => ({
      id: item.id,
      label: item.label,
      icon: item.icon,
      active: item.isActive(ctx),
      onSelect: item.onSelect(handlers),
    }))
}
