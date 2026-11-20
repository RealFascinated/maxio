<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";

	type Props = DropdownMenuPrimitive.ItemProps & {
		variant?: "default" | "destructive";
		/** Right-aligned hint, e.g. a key combo. */
		shortcut?: Snippet;
	};

	let {
		ref = $bindable(null),
		class: className,
		variant = "default",
		shortcut,
		children,
		...restProps
	}: Props = $props();
</script>

<DropdownMenuPrimitive.Item
	bind:ref
	data-slot="dropdown-menu-item"
	class={cn(
		"flex h-7 cursor-pointer items-center gap-2 rounded-control px-2 text-sm outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 data-[highlighted]:bg-hover [&_svg]:size-3.5 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
		variant === "destructive" && "text-danger data-[highlighted]:bg-hover [&_svg]:text-danger",
		className
	)}
	{...restProps}
>
	{@render children?.()}
	{#if shortcut}
		<span class="ml-auto flex items-center gap-1 pl-4">{@render shortcut()}</span>
	{/if}
</DropdownMenuPrimitive.Item>
