<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLButtonAttributes } from "svelte/elements";

	type Props = WithElementRef<HTMLButtonAttributes> & {
		checked?: boolean;
	};

	let {
		ref = $bindable(null),
		class: className,
		checked = false,
		disabled,
		type = "button",
		...restProps
	}: Props = $props();

	const state = $derived(checked ? "checked" : "unchecked");
</script>

<button
	bind:this={ref}
	data-slot="switch"
	data-state={state}
	role="switch"
	aria-checked={checked}
	{type}
	{disabled}
	class={cn(
		"inline-flex h-4 w-7 shrink-0 cursor-pointer items-center rounded-control border border-border-strong bg-inset p-px outline-none transition-colors disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-accent data-[state=checked]:bg-accent",
		className
	)}
	{...restProps}
>
	<span
		data-slot="switch-thumb"
		data-state={state}
		class="pointer-events-none block size-3 rounded-control bg-muted-foreground transition-transform data-[state=checked]:translate-x-3 data-[state=checked]:bg-white"
	></span>
</button>
