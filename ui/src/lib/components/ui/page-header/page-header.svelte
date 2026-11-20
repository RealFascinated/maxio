<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import type { Snippet } from "svelte";

	type Props = WithElementRef<HTMLAttributes<HTMLElement>> & {
		title: string;
		breadcrumbs?: Snippet;
		actions?: Snippet;
	};

	let { ref = $bindable(null), class: className, title, breadcrumbs, actions, ...restProps }: Props = $props();
</script>

<header
	bind:this={ref}
	data-slot="page-header"
	class={cn("flex min-h-8 flex-wrap items-center justify-between gap-x-4 gap-y-2", className)}
	{...restProps}
>
	<div class="flex min-w-0 items-center gap-2">
		{@render breadcrumbs?.()}
		<h1 class="truncate">{title}</h1>
	</div>
	{#if actions}
		<div class="flex shrink-0 items-center gap-2">{@render actions()}</div>
	{/if}
</header>
