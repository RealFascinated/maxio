<script lang="ts">
  import ChevronRight from 'lucide-svelte/icons/chevron-right'
  import Keyboard from 'lucide-svelte/icons/keyboard'
  import LogOut from 'lucide-svelte/icons/log-out'
  import Menu from 'lucide-svelte/icons/menu'
  import Monitor from 'lucide-svelte/icons/monitor'
  import Moon from 'lucide-svelte/icons/moon'
  import Search from 'lucide-svelte/icons/search'
  import Sun from 'lucide-svelte/icons/sun'
  import { buildBreadcrumbs } from '$lib/components/app/topbar/breadcrumbs'
  import type { ThemeMode } from '$lib/components/app/sidebar/theme'
  import { Button } from '$lib/components/ui/button'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { Kbd } from '$lib/components/ui/kbd'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { comboKeys } from '$lib/shortcuts'
  import { appUi } from '$lib/app-ui.svelte'
  import { cn } from '$lib/utils'

  interface Props {
    pathname: string
    username?: string
    isRoot?: boolean
    themeMode: ThemeMode
    isDark: boolean
    onThemeChange: (mode: ThemeMode) => void
    onLogout: () => void
  }

  let { pathname, username, isRoot = false, themeMode, isDark, onThemeChange, onLogout }: Props = $props()

  const crumbs = $derived(buildBreadcrumbs(pathname))
  const displayName = $derived(username?.trim() || 'Signed in')
  const themeOptions: { mode: ThemeMode; label: string; icon: typeof Sun }[] = [
    { mode: 'light', label: 'Light', icon: Sun },
    { mode: 'system', label: 'System', icon: Monitor },
    { mode: 'dark', label: 'Dark', icon: Moon },
  ]
</script>

<header class="flex h-10 shrink-0 items-center gap-2 border-b border-border bg-panel px-3">
  <Button
    variant="ghost"
    size="icon-xs"
    class="lg:hidden"
    aria-label="Open navigation"
    onclick={() => (appUi.mobileNavOpen = true)}
  >
    <Menu />
  </Button>

  <nav aria-label="Breadcrumb" class="min-w-0 flex-1">
    <ol class="flex items-center gap-1 text-sm">
      {#each crumbs as crumb, index (index)}
        {#if index > 0}<ChevronRight class="size-3 shrink-0 text-subtle" aria-hidden="true" />{/if}
        <li class={cn('min-w-0 truncate', index === crumbs.length - 1 ? 'font-medium text-foreground' : 'text-muted-foreground')}>
          {#if crumb.href}
            <a href={crumb.href} class="rounded-control hover:text-foreground">{crumb.label}</a>
          {:else}
            <span aria-current={index === crumbs.length - 1 ? 'page' : undefined}>{crumb.label}</span>
          {/if}
        </li>
      {/each}
    </ol>
  </nav>

  <button
    type="button"
    onclick={() => (appUi.paletteOpen = true)}
    class="hidden h-7 w-64 items-center gap-2 rounded-control border border-border-strong bg-inset px-2 text-sm text-subtle transition-colors hover:border-subtle sm:flex"
  >
    <Search class="size-3.5 shrink-0" aria-hidden="true" />
    <span class="flex-1 truncate text-left">Search or jump to…</span>
    <span class="flex gap-0.5">{#each comboKeys('mod+k') as key}<Kbd>{key}</Kbd>{/each}</span>
  </button>
  <Button variant="ghost" size="icon-xs" class="sm:hidden" aria-label="Search" onclick={() => (appUi.paletteOpen = true)}>
    <Search />
  </Button>

  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          variant="ghost"
          size="icon-xs"
          aria-label="Toggle theme"
          onclick={() => onThemeChange(isDark ? 'light' : 'dark')}
        >
          {#if isDark}<Sun />{:else}<Moon />{/if}
        </Button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>Switch to {isDark ? 'light' : 'dark'} theme</Tooltip.Content>
  </Tooltip.Root>

  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <button
          {...props}
          type="button"
          class="grid size-6 place-items-center rounded-control border border-border-strong bg-raised text-xs font-medium uppercase transition-colors hover:border-subtle"
          aria-label="Account menu"
        >
          {displayName.charAt(0)}
        </button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content class="w-52">
      <div class="flex items-center gap-2 px-2 py-1.5">
        <span class="min-w-0 flex-1 truncate text-sm font-medium">{displayName}</span>
        {#if isRoot}<span class="text-xs text-subtle">root</span>{/if}
      </div>
      <DropdownMenu.Separator />
      <DropdownMenu.Group>
        <DropdownMenu.Label>Theme</DropdownMenu.Label>
        <DropdownMenu.RadioGroup value={themeMode} onValueChange={(mode) => onThemeChange(mode as ThemeMode)}>
          {#each themeOptions as option (option.mode)}
            <DropdownMenu.RadioItem value={option.mode}>
              <option.icon class="size-3.5 text-muted-foreground" aria-hidden="true" />
              {option.label}
            </DropdownMenu.RadioItem>
          {/each}
        </DropdownMenu.RadioGroup>
      </DropdownMenu.Group>
      <DropdownMenu.Separator />
      <DropdownMenu.Item onSelect={() => (appUi.shortcutsOpen = true)}>
        <Keyboard />
        Keyboard shortcuts
        {#snippet shortcut()}<Kbd>?</Kbd>{/snippet}
      </DropdownMenu.Item>
      <DropdownMenu.Item onSelect={onLogout}>
        <LogOut />
        Sign out
      </DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</header>
