<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import { base } from '$lib/base'
  import SidebarNavItem from '$lib/components/app/sidebar/SidebarNavItem.svelte'
  import type { SidebarNavEntry } from '$lib/components/app/sidebar/navigation'

  interface Props {
    open?: boolean
    navItems: SidebarNavEntry[]
  }

  let { open = $bindable(false), navItems }: Props = $props()
</script>

<DialogPrimitive.Root bind:open>
  <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay
      class="fixed inset-0 z-50 bg-black/50 data-[state=open]:animate-[ui-fade-in_120ms_ease-out]"
    />
    <DialogPrimitive.Content
      aria-label="Navigation"
      onOpenAutoFocus={(event) => event.preventDefault()}
      class="fixed inset-y-0 left-0 z-50 flex w-56 flex-col border-r border-border bg-panel outline-none data-[state=open]:animate-[ui-slide-in-left_160ms_ease-out]"
    >
      <DialogPrimitive.Title class="sr-only">Navigation</DialogPrimitive.Title>
      <div class="flex h-10 shrink-0 items-center gap-2 border-b border-border px-3">
        <img src={`${base}/maxio.png`} alt="" class="size-5" aria-hidden="true" />
        <span class="text-sm font-semibold tracking-tight">MaxIO</span>
      </div>
      <div class="flex flex-col gap-1 p-2" role="list">
        {#each navItems as item (item.id)}
          <SidebarNavItem
            label={item.label}
            icon={item.icon}
            href={item.href}
            active={item.active}
            onNavigate={() => (open = false)}
          />
        {/each}
      </div>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
