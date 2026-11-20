<script lang="ts" module>
	export type SegmentedOption = {
		value: string;
		label?: string;
		icon?: import("$lib/lucide").LucideIcon;
		disabled?: boolean;
	};
</script>

<script lang="ts">
	import { cn } from "$lib/utils.js";

	type Props = {
		options: SegmentedOption[];
		value?: string;
		size?: "xs" | "md";
		class?: string;
		"aria-label"?: string;
		onValueChange?: (value: string) => void;
	};

	let {
		options,
		value = $bindable(),
		size = "md",
		class: className,
		"aria-label": ariaLabel,
		onValueChange,
	}: Props = $props();

	function select(option: SegmentedOption) {
		if (option.disabled) return;
		value = option.value;
		onValueChange?.(option.value);
	}
</script>

<div
	role="group"
	aria-label={ariaLabel}
	data-slot="segmented-control"
	class={cn("inline-flex items-center gap-px rounded-control border border-border-strong bg-inset p-0.5", className)}
>
	{#each options as option (option.value)}
		{@const Icon = option.icon}
		<button
			type="button"
			aria-pressed={option.value === value}
			aria-label={option.label ? undefined : option.value}
			disabled={option.disabled}
			class={cn(
				"inline-flex items-center justify-center gap-1.5 rounded-control px-2 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50 aria-pressed:bg-raised aria-pressed:text-foreground aria-pressed:shadow-[0_0_0_1px_var(--border-strong)]",
				size === "xs" ? "h-5" : "h-6"
			)}
			onclick={() => select(option)}
		>
			{#if Icon}<Icon class="size-3.5" />{/if}
			{option.label}
		</button>
	{/each}
</div>
