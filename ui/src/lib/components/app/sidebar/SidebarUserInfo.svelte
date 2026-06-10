<script lang="ts">
  import CircleUser from 'lucide-svelte/icons/circle-user'

  interface Props {
    username?: string
    isRoot?: boolean
    collapsed?: boolean
  }

  let { username, isRoot = false, collapsed = false }: Props = $props()

  const displayName = $derived(username?.trim() || 'Signed in')
  const tooltip = $derived(isRoot ? `${displayName} (root)` : displayName)
</script>

{#if collapsed}
  <div
    class="mx-auto flex size-8 items-center justify-center rounded-sm text-muted-foreground"
    title={tooltip}
    aria-label={tooltip}
  >
    <CircleUser class="size-4 shrink-0" aria-hidden="true" />
  </div>
{:else}
  <div class="flex min-h-7 w-full items-center gap-3 rounded-sm px-2 py-1 text-sm text-muted-foreground">
    <CircleUser class="size-4 shrink-0" aria-hidden="true" />
    <div class="flex min-w-0 items-center gap-2">
      <span class="truncate font-medium text-foreground">{displayName}</span>
      {#if isRoot}
        <span
          class="shrink-0 rounded-sm bg-coollabs-50 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-coollabs-200 dark:bg-coolgray-200 dark:text-warning"
        >
          Root
        </span>
      {/if}
    </div>
  </div>
{/if}
