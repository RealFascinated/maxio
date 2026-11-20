<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import Check from "lucide-svelte/icons/check";
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";

	type Props = Omit<DropdownMenuPrimitive.RadioItemProps, "children"> & {
		children?: Snippet;
	};

	let { ref = $bindable(null), class: className, children: label, ...restProps }: Props = $props();
</script>

<DropdownMenuPrimitive.RadioItem
	bind:ref
	data-slot="dropdown-menu-radio-item"
	class={cn(
		"flex h-7 cursor-pointer items-center gap-2 rounded-control px-2 text-sm outline-none select-none data-[disabled]:opacity-50 data-[highlighted]:bg-hover [&_svg]:size-3.5 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
		className
	)}
	{...restProps}
>
	{#snippet children({ checked })}
		{@render label?.()}
		{#if checked}<Check class="ml-auto !text-accent" />{/if}
	{/snippet}
</DropdownMenuPrimitive.RadioItem>
