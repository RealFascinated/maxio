<script lang="ts" module>
	export type SelectOption = { value: string; label: string; disabled?: boolean };
</script>

<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";
	import Check from "lucide-svelte/icons/check";
	import ChevronDown from "lucide-svelte/icons/chevron-down";
	import { cn } from "$lib/utils.js";

	type Props = {
		value?: string;
		options: SelectOption[];
		placeholder?: string;
		disabled?: boolean;
		size?: "xs" | "md" | "lg";
		class?: string;
		"aria-label"?: string;
		onValueChange?: (value: string) => void;
	};

	let {
		value = $bindable(""),
		options,
		placeholder = "Select",
		disabled = false,
		size = "md",
		class: className,
		"aria-label": ariaLabel,
		onValueChange,
	}: Props = $props();

	const selected = $derived(options.find((option) => option.value === value));
</script>

<SelectPrimitive.Root
	type="single"
	bind:value
	{disabled}
	items={options}
	onValueChange={(next) => onValueChange?.(next)}
>
	<SelectPrimitive.Trigger
		data-slot="select-trigger"
		aria-label={ariaLabel}
		class={cn(
			"inline-flex w-full min-w-0 items-center justify-between gap-2 rounded-control border border-border-strong bg-inset px-2 text-sm outline-none transition-colors hover:border-subtle focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-0 disabled:cursor-not-allowed disabled:opacity-50 data-[state=open]:border-accent",
			size === "xs" ? "h-6 text-xs" : size === "lg" ? "h-8" : "h-7",
			className
		)}
	>
		<span class={cn("truncate", !selected && "text-subtle")}>{selected?.label ?? placeholder}</span>
		<ChevronDown class="size-3.5 shrink-0 text-subtle" />
	</SelectPrimitive.Trigger>
	<SelectPrimitive.Portal>
		<SelectPrimitive.Content
			sideOffset={4}
			class="z-[60] max-h-72 min-w-[var(--bits-select-anchor-width)] overflow-hidden rounded-surface border border-border-strong bg-raised p-1 text-sm shadow-pop outline-none data-[state=open]:animate-[ui-pop-in_100ms_ease-out]"
		>
			<SelectPrimitive.Viewport>
				{#each options as option (option.value)}
					<SelectPrimitive.Item
						value={option.value}
						label={option.label}
						disabled={option.disabled}
						class="flex h-7 cursor-pointer items-center justify-between gap-2 rounded-control px-2 outline-none select-none data-[disabled]:opacity-50 data-[highlighted]:bg-hover"
					>
						{#snippet children({ selected })}
							<span class="truncate">{option.label}</span>
							{#if selected}<Check class="size-3.5 shrink-0 text-accent" />{/if}
						{/snippet}
					</SelectPrimitive.Item>
				{/each}
			</SelectPrimitive.Viewport>
		</SelectPrimitive.Content>
	</SelectPrimitive.Portal>
</SelectPrimitive.Root>
