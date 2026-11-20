<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	type Props = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		/** 0–100. */
		value: number;
		/** Switches to warning/danger colour past these percentages. */
		warnAt?: number;
		dangerAt?: number;
		size?: "sm" | "md";
	};

	let {
		ref = $bindable(null),
		class: className,
		value,
		warnAt,
		dangerAt,
		size = "sm",
		...restProps
	}: Props = $props();

	const percent = $derived(Math.min(100, Math.max(0, value)));
	const fill = $derived(
		dangerAt !== undefined && percent >= dangerAt
			? "bg-danger"
			: warnAt !== undefined && percent >= warnAt
				? "bg-warning"
				: "bg-accent"
	);
</script>

<div
	bind:this={ref}
	data-slot="progress-bar"
	role="progressbar"
	aria-valuemin={0}
	aria-valuemax={100}
	aria-valuenow={Math.round(percent)}
	class={cn("w-full overflow-hidden rounded-control bg-hover", size === "sm" ? "h-1" : "h-1.5", className)}
	{...restProps}
>
	<div class={cn("h-full rounded-control transition-[width] duration-300", fill)} style:width="{percent}%"></div>
</div>
