<script lang="ts">
  import { Dialog } from '$lib/components/ui/dialog'
  import { Kbd } from '$lib/components/ui/kbd'
  import { comboKeys, listShortcuts } from '$lib/shortcuts'
  import { appUi } from '$lib/app-ui.svelte'

  const groups = $derived.by(() => {
    if (!appUi.shortcutsOpen) return []
    const byGroup = new Map<string, ReturnType<typeof listShortcuts>>()
    for (const shortcut of listShortcuts()) {
      byGroup.set(shortcut.group, [...(byGroup.get(shortcut.group) ?? []), shortcut])
    }
    return [...byGroup]
  })
</script>

<Dialog bind:open={appUi.shortcutsOpen} title="Keyboard shortcuts">
  <div class="flex flex-col gap-4">
    {#each groups as [group, shortcuts] (group)}
      <section>
        <h3 class="mb-1 text-xs font-medium text-subtle">{group}</h3>
        <ul class="divide-y divide-border">
          {#each shortcuts as shortcut (shortcut.combo)}
            <li class="flex h-8 items-center justify-between gap-4">
              <span>{shortcut.description}</span>
              <span class="flex items-center gap-1 text-subtle">
                {#each shortcut.combo.split(' ') as step, index}
                  {#if index > 0}<span class="text-xs">then</span>{/if}
                  {#each comboKeys(step) as key}<Kbd>{key}</Kbd>{/each}
                {/each}
              </span>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  </div>
</Dialog>
