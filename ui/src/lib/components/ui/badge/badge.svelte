<script lang="ts" module>
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import { type VariantProps, tv } from "tailwind-variants";

	/** Status dot. With `label`, renders dot + text. */
	export const badgeVariants = tv({
		base: "inline-block size-2 shrink-0 rounded-full",
		variants: {
			variant: {
				success: "bg-success",
				warning: "bg-warning",
				error: "bg-danger",
				danger: "bg-danger",
				info: "bg-info",
				neutral: "bg-subtle",
			},
		},
		defaultVariants: {
			variant: "success",
		},
	});

	export type BadgeVariant = VariantProps<typeof badgeVariants>["variant"];

	export type BadgeProps = WithElementRef<HTMLAttributes<HTMLSpanElement>> & {
		variant?: BadgeVariant;
		label?: string;
	};
</script>

<script lang="ts">
	let {
		ref = $bindable(null),
		class: className,
		variant = "success",
		label,
		...restProps
	}: BadgeProps = $props();

	const textColor = $derived(
		variant === "success"
			? "text-success"
			: variant === "error" || variant === "danger"
				? "text-danger"
				: variant === "warning"
					? "text-warning"
					: variant === "info"
						? "text-info"
						: "text-muted-foreground"
	);
</script>

{#if label}
	<span class="inline-flex items-center gap-1.5">
		<span
			bind:this={ref}
			data-slot="badge"
			class={cn(badgeVariants({ variant }), className)}
			{...restProps}
		></span>
		<span class="text-xs font-medium {textColor}">{label}</span>
	</span>
{:else}
	<span
		bind:this={ref}
		data-slot="badge"
		class={cn(badgeVariants({ variant }), className)}
		{...restProps}
	></span>
{/if}
