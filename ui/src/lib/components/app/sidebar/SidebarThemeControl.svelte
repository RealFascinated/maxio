<script lang="ts">
  import Sun from 'lucide-svelte/icons/sun'
  import Moon from 'lucide-svelte/icons/moon'
  import Monitor from 'lucide-svelte/icons/monitor'
  import type { ThemeMode } from '$lib/components/app/sidebar/theme'

  interface ThemeOption {
    mode: ThemeMode
    label: string
    icon: typeof Sun
  }

  interface Props {
    collapsed?: boolean
    themeMode: ThemeMode
    onThemeChange: (mode: ThemeMode) => void
    onCycleTheme: () => void
  }

  let { collapsed = false, themeMode, onThemeChange, onCycleTheme }: Props = $props()

  const themeOptions: ThemeOption[] = [
    { mode: 'light', label: 'Light', icon: Sun },
    { mode: 'system', label: 'System', icon: Monitor },
    { mode: 'dark', label: 'Dark', icon: Moon },
  ]

  const currentTheme = $derived(
    themeOptions.find((option) => option.mode === themeMode) ?? themeOptions[1]!,
  )
</script>

{#if collapsed}
  <button
    type="button"
    onclick={onCycleTheme}
    class="mx-auto flex size-8 items-center justify-center rounded-sm text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:focus-visible:ring-warning"
    aria-label={`Theme: ${currentTheme.label}. Click to switch theme.`}
    title={`Theme: ${currentTheme.label}`}
  >
    <currentTheme.icon class="size-4 shrink-0" aria-hidden="true" />
  </button>
{:else}
  <div
    class="flex min-h-7 w-full items-center justify-between gap-3 rounded-sm px-2 py-1 text-sm text-muted-foreground"
  >
    <span class="whitespace-nowrap">Theme</span>
    <div
      class="inline-flex items-center gap-0.5 rounded-sm bg-neutral-100 p-0.5 dark:bg-coolgray-200"
      role="group"
      aria-label="Theme"
    >
      {#each themeOptions as option (option.mode)}
        {@const Icon = option.icon}
        <button
          type="button"
          onclick={() => onThemeChange(option.mode)}
          class={`grid size-6 place-items-center rounded-sm text-neutral-500 transition-colors hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-white dark:focus-visible:ring-warning ${themeMode === option.mode ? 'bg-white text-coollabs shadow-sm dark:bg-base dark:text-warning' : ''}`}
          aria-label={`Use ${option.label} theme`}
          aria-pressed={themeMode === option.mode}
          title={option.label}
        >
          <Icon class="size-4" aria-hidden="true" />
        </button>
      {/each}
    </div>
  </div>
{/if}
