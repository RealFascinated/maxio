<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLElement>> = $props();
</script>

<kbd
	bind:this={ref}
	data-slot="kbd"
	class={cn(
		"inline-flex h-4 min-w-4 items-center justify-center rounded-control border border-border-strong bg-inset px-1 font-mono text-[10px] leading-none text-muted-foreground",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</kbd>
