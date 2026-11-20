<script lang="ts" module>
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import { type VariantProps, tv } from "tailwind-variants";

	export const cardVariants = tv({
		base: "group flex flex-col gap-3 rounded-surface border border-border bg-panel py-3 text-card-foreground transition-colors",
		variants: {
			variant: {
				default: "",
				box: "cursor-pointer hover:border-border-strong hover:bg-raised",
				/** @deprecated alias of box */
				coolbox: "cursor-pointer hover:border-border-strong hover:bg-raised",
			},
		},
		defaultVariants: {
			variant: "default",
		},
	});

	export type CardVariant = VariantProps<typeof cardVariants>["variant"];

	export type CardProps = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		variant?: CardVariant;
	};
</script>

<script lang="ts">
	let {
		ref = $bindable(null),
		class: className,
		variant = "default",
		children,
		...restProps
	}: CardProps = $props();
</script>

<div
	bind:this={ref}
	data-slot="card"
	class={cn(cardVariants({ variant }), className)}
	{...restProps}
>
	{@render children?.()}
</div>
