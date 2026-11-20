<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import type { Snippet } from "svelte";

	type Props = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		label: string;
		value: string | number;
		/** Small trailing text next to the value, e.g. a unit or delta. */
		unit?: string;
		tone?: "default" | "success" | "warning" | "danger";
		/** Optional sparkline or bar rendered under the value. */
		chart?: Snippet;
	};

	let {
		ref = $bindable(null),
		class: className,
		label,
		value,
		unit,
		tone = "default",
		chart,
		...restProps
	}: Props = $props();

	const toneClass = {
		default: "text-foreground",
		success: "text-success",
		warning: "text-warning",
		danger: "text-danger",
	};
</script>

<div
	bind:this={ref}
	data-slot="stat-tile"
	class={cn("flex min-w-0 flex-col gap-1 rounded-surface border border-border bg-panel px-3 py-2.5", className)}
	{...restProps}
>
	<span class="truncate text-xs text-muted-foreground">{label}</span>
	<div class="flex items-baseline gap-1.5">
		<span class={cn("num text-xl leading-6 font-semibold tracking-tight", toneClass[tone])}>{value}</span>
		{#if unit}<span class="text-xs text-subtle">{unit}</span>{/if}
	</div>
	{#if chart}<div class="mt-1">{@render chart()}</div>{/if}
</div>
