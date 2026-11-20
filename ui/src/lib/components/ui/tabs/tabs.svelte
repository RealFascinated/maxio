<script lang="ts" module>
	export type TabItem = {
		value: string;
		label: string;
		/** When set the tab renders as a link, so the active tab can live in the URL. */
		href?: string;
		count?: number;
		disabled?: boolean;
	};
</script>

<script lang="ts">
	import { cn } from "$lib/utils.js";

	type Props = {
		items: TabItem[];
		value?: string;
		class?: string;
		"aria-label"?: string;
		onValueChange?: (value: string) => void;
	};

	let { items, value = $bindable(), class: className, "aria-label": ariaLabel, onValueChange }: Props = $props();

	function select(item: TabItem) {
		if (item.disabled) return;
		value = item.value;
		onValueChange?.(item.value);
	}

	function handleKeydown(event: KeyboardEvent) {
		const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
		if (!keys.includes(event.key)) return;
		const enabled = items.filter((item) => !item.disabled);
		const index = enabled.findIndex((item) => item.value === value);
		const next =
			event.key === "Home"
				? 0
				: event.key === "End"
					? enabled.length - 1
					: (index + (event.key === "ArrowRight" ? 1 : -1) + enabled.length) % enabled.length;
		const target = enabled[next];
		if (!target) return;
		event.preventDefault();
		const element = (event.currentTarget as HTMLElement).querySelector<HTMLElement>(
			`[data-value="${CSS.escape(target.value)}"]`,
		);
		element?.focus();
		if (target.href) element?.click();
		else select(target);
	}

	const itemClass =
		"relative -mb-px inline-flex h-8 items-center gap-1.5 border-b-2 border-transparent px-2.5 text-sm text-muted-foreground transition-colors hover:text-foreground aria-selected:border-accent aria-selected:text-foreground aria-disabled:pointer-events-none aria-disabled:opacity-50";
</script>

<div
	role="tablist"
	aria-label={ariaLabel}
	data-slot="tabs"
	tabindex="-1"
	class={cn("flex items-center gap-1 border-b border-border", className)}
	onkeydown={handleKeydown}
>
	{#each items as item (item.value)}
		{@const active = item.value === value}
		{#if item.href}
			<a
				role="tab"
				href={item.href}
				data-value={item.value}
				aria-selected={active}
				aria-disabled={item.disabled}
				tabindex={active ? 0 : -1}
				class={itemClass}
				onclick={() => select(item)}
			>
				{item.label}
				{#if item.count !== undefined}<span class="num text-xs text-subtle">{item.count}</span>{/if}
			</a>
		{:else}
			<button
				type="button"
				role="tab"
				data-value={item.value}
				aria-selected={active}
				aria-disabled={item.disabled}
				tabindex={active ? 0 : -1}
				class={itemClass}
				onclick={() => select(item)}
			>
				{item.label}
				{#if item.count !== undefined}<span class="num text-xs text-subtle">{item.count}</span>{/if}
			</button>
		{/if}
	{/each}
</div>
