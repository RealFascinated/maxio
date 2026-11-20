<script lang="ts" module>
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import { type VariantProps, tv } from "tailwind-variants";

	/** Flat neutral surface; status shows only as the dot/icon colour. */
	export const calloutVariants = tv({
		base: "flex items-start gap-2 rounded-surface border border-border bg-panel px-3 py-2 text-sm text-foreground",
		variants: {
			type: {
				warning: "[&_.callout-icon]:text-warning",
				danger: "[&_.callout-icon]:text-danger",
				info: "[&_.callout-icon]:text-info",
				success: "[&_.callout-icon]:text-success",
			},
		},
		defaultVariants: {
			type: "info",
		},
	});

	export type CalloutType = VariantProps<typeof calloutVariants>["type"];

	export type CalloutProps = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		type?: CalloutType;
		title?: string;
		icon?: import("svelte").Snippet;
	};

	const dotColor = {
		warning: "bg-warning",
		danger: "bg-danger",
		info: "bg-info",
		success: "bg-success",
	};
</script>

<script lang="ts">
	let {
		ref = $bindable(null),
		class: className,
		type = "info",
		title,
		icon,
		children,
		...restProps
	}: CalloutProps = $props();
</script>

<div
	bind:this={ref}
	data-slot="callout"
	role={type === "danger" || type === "warning" ? "alert" : "status"}
	class={cn(calloutVariants({ type }), className)}
	{...restProps}
>
	{#if icon}
		<div class="callout-icon mt-px shrink-0 [&_svg]:size-4">{@render icon()}</div>
	{:else}
		<span class="mt-[7px] size-2 shrink-0 rounded-full {dotColor[type ?? 'info']}" aria-hidden="true"></span>
	{/if}
	<div class="min-w-0 flex-1">
		{#if title}
			<div class="callout-title font-medium">{title}</div>
		{/if}
		<div class="callout-body text-muted-foreground">{@render children?.()}</div>
	</div>
</div>
