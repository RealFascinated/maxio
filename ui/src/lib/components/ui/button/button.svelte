<script lang="ts" module>
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from "svelte/elements";
	import { type VariantProps, tv } from "tailwind-variants";

	export const buttonVariants = tv({
		base: "inline-flex shrink-0 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap rounded-control border border-transparent text-sm font-medium outline-none transition-colors select-none disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
		variants: {
			variant: {
				default: "bg-accent text-accent-foreground hover:bg-accent-hover",
				secondary: "bg-raised text-foreground border-border hover:bg-hover",
				outline:
					"border-border-strong bg-transparent text-foreground hover:bg-hover",
				ghost: "text-muted-foreground hover:bg-hover hover:text-foreground",
				destructive:
					"bg-danger text-white hover:bg-danger/85",
				link: "h-auto px-0 text-accent underline-offset-4 hover:underline",
				/** @deprecated alias of default, kept until pages are migrated */
				highlighted: "bg-accent text-accent-foreground hover:bg-accent-hover",
				/** @deprecated alias of default, kept until pages are migrated */
				brand: "bg-accent text-accent-foreground hover:bg-accent-hover",
			},
			size: {
				xs: "h-6 px-2 text-xs",
				sm: "h-6 px-2 text-xs",
				md: "h-7 px-2.5",
				default: "h-7 px-2.5",
				lg: "h-8 px-3",
				icon: "size-7",
				"icon-xs": "size-6",
				"icon-sm": "size-6",
				"icon-lg": "size-8",
			},
		},
		defaultVariants: {
			variant: "default",
			size: "md",
		},
	});

	export type ButtonVariant = VariantProps<typeof buttonVariants>["variant"];
	export type ButtonSize = VariantProps<typeof buttonVariants>["size"];

	export type ButtonProps = WithElementRef<HTMLButtonAttributes> &
		WithElementRef<HTMLAnchorAttributes> & {
			variant?: ButtonVariant;
			size?: ButtonSize;
			/** Shows a spinner and disables the button. */
			loading?: boolean;
		};
</script>

<script lang="ts">
	import Loader from "lucide-svelte/icons/loader-circle";

	let {
		class: className,
		variant = "default",
		size = "md",
		ref = $bindable(null),
		href = undefined,
		type = "button",
		disabled,
		loading = false,
		children,
		...restProps
	}: ButtonProps = $props();

	const isDisabled = $derived(disabled || loading);
</script>

{#if href}
	<a
		bind:this={ref}
		data-slot="button"
		class={cn(buttonVariants({ variant, size }), className)}
		href={isDisabled ? undefined : href}
		aria-disabled={isDisabled}
		role={isDisabled ? "link" : undefined}
		tabindex={isDisabled ? -1 : undefined}
		{...restProps}
	>
		{#if loading}<Loader class="animate-spin" />{/if}
		{@render children?.()}
	</a>
{:else}
	<button
		bind:this={ref}
		data-slot="button"
		class={cn(buttonVariants({ variant, size }), className)}
		{type}
		disabled={isDisabled}
		aria-busy={loading || undefined}
		{...restProps}
	>
		{#if loading}<Loader class="animate-spin" />{/if}
		{@render children?.()}
	</button>
{/if}
