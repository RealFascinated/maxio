<script lang="ts">
	import type { HTMLTableAttributes } from "svelte/elements";
	import { cn, type WithElementRef } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLTableAttributes> = $props();
</script>

<div data-slot="table-container" class="relative w-full overflow-auto rounded-surface border border-border bg-panel">
	<table
		bind:this={ref}
		data-slot="table"
		class={cn(
			"w-full border-separate border-spacing-0 caption-bottom text-left text-sm",
			className
		)}
		{...restProps}
	>
		{@render children?.()}
	</table>
</div>
