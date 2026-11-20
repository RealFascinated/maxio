<script lang="ts">
  import { onMount } from 'svelte'
  import { goto } from '$app/navigation'
  import { createQuery } from '@tanstack/svelte-query'
  import BarChart2 from 'lucide-svelte/icons/bar-chart-2'
  import Database from 'lucide-svelte/icons/database'
  import Keyboard from 'lucide-svelte/icons/keyboard'
  import LogOut from 'lucide-svelte/icons/log-out'
  import Plus from 'lucide-svelte/icons/plus'
  import Settings from 'lucide-svelte/icons/settings'
  import Shield from 'lucide-svelte/icons/shield'
  import SunMoon from 'lucide-svelte/icons/sun-moon'
  import UsersIcon from 'lucide-svelte/icons/users'
  import { listBuckets } from '$lib/api/buckets'
  import { bucketKeys } from '$lib/api/keys'
  import ShortcutsDialog from '$lib/components/app/commands/ShortcutsDialog.svelte'
  import { CommandPalette, type CommandItem } from '$lib/components/ui/command-palette'
  import { appUi } from '$lib/app-ui.svelte'
  import { routes } from '$lib/navigation'
  import { register } from '$lib/shortcuts'

  interface Props {
    isRoot: boolean
    onLogout: () => void
    onToggleTheme: () => void
    onToggleSidebar: () => void
  }

  let { isRoot, onLogout, onToggleTheme, onToggleSidebar }: Props = $props()

  const bucketsQuery = createQuery(() => ({ queryKey: bucketKeys.list(), queryFn: listBuckets }))

  const go = (href: string) => () => goto(href)

  const items = $derived.by<CommandItem[]>(() => {
    const navigation: CommandItem[] = [
      { id: 'go-buckets', label: 'Go to Buckets', group: 'Navigate', icon: Database, shortcut: 'g b', onSelect: go(routes.home()) },
    ]
    if (isRoot) {
      navigation.push(
        { id: 'go-users', label: 'Go to Users', group: 'Navigate', icon: UsersIcon, shortcut: 'g u', onSelect: go(routes.users()) },
        { id: 'go-policies', label: 'Go to Policies', group: 'Navigate', icon: Shield, shortcut: 'g p', onSelect: go(routes.policies()) },
        { id: 'go-metrics', label: 'Go to Metrics', group: 'Navigate', icon: BarChart2, shortcut: 'g m', onSelect: go(routes.metrics()) },
        { id: 'go-settings', label: 'Go to Settings', group: 'Navigate', icon: Settings, shortcut: 'g s', onSelect: go(routes.serverSettings()) },
      )
    }
    const actions: CommandItem[] = [
      // Pages open their create dialog when `?new` is present.
      { id: 'create-bucket', label: 'Create bucket', group: 'Actions', icon: Plus, keywords: 'new add', onSelect: go(`${routes.home()}?new`) },
      { id: 'toggle-theme', label: 'Toggle theme', group: 'Actions', icon: SunMoon, keywords: 'dark light', onSelect: onToggleTheme },
      { id: 'shortcuts', label: 'Keyboard shortcuts', group: 'Actions', icon: Keyboard, shortcut: '?', onSelect: () => (appUi.shortcutsOpen = true) },
      { id: 'sign-out', label: 'Sign out', group: 'Actions', icon: LogOut, keywords: 'logout', onSelect: onLogout },
    ]
    const buckets: CommandItem[] = (bucketsQuery.data?.buckets ?? []).map((bucket) => ({
      id: `bucket:${bucket.name}`,
      label: bucket.name,
      group: 'Buckets',
      icon: Database,
      onSelect: go(routes.bucket(bucket.name)),
    }))
    return [...navigation, ...actions, ...buckets]
  })

  onMount(() => {
    const openPalette = () => (appUi.paletteOpen = true)
    const chords: [string, string, string, boolean][] = [
      ['g b', 'Go to Buckets', routes.home(), false],
      ['g u', 'Go to Users', routes.users(), true],
      ['g p', 'Go to Policies', routes.policies(), true],
      ['g m', 'Go to Metrics', routes.metrics(), true],
      ['g s', 'Go to Settings', routes.serverSettings(), true],
    ]
    const unregister = [
      register(['mod+k'], openPalette, { description: 'Open command palette', group: 'General' }),
      register('/', openPalette, { description: 'Search', group: 'General' }),
      register('?', () => (appUi.shortcutsOpen = true), { description: 'Show keyboard shortcuts', group: 'General' }),
      register('[', onToggleSidebar, { description: 'Toggle sidebar', group: 'General' }),
      ...chords.map(([combo, description, href, rootOnly]) =>
        register(combo, () => (!rootOnly || isRoot) && goto(href), { description, group: 'Navigation' }),
      ),
    ]
    return () => unregister.forEach((off) => off())
  })
</script>

<CommandPalette bind:open={appUi.paletteOpen} {items} />
<ShortcutsDialog />
