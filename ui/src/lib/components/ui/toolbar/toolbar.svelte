<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import type { Snippet } from "svelte";

	type Props = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		/** Right-aligned controls (view toggles, counts, bulk actions). */
		end?: Snippet;
	};

	let { ref = $bindable(null), class: className, children, end, ...restProps }: Props = $props();
</script>

<div
	bind:this={ref}
	data-slot="toolbar"
	role="toolbar"
	class={cn("flex min-h-8 flex-wrap items-center gap-2", className)}
	{...restProps}
>
	<div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">{@render children?.()}</div>
	{#if end}
		<div class="flex shrink-0 items-center gap-2">{@render end()}</div>
	{/if}
</div>
