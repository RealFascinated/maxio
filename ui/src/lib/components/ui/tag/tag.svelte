<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLSpanElement>> = $props();
</script>

<span
	bind:this={ref}
	data-slot="tag"
	class={cn(
		"inline-flex h-5 items-center rounded-control border border-border bg-inset px-1.5 text-xs text-muted-foreground",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</span>
