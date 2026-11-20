<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import type { Snippet } from "svelte";

	type Props = WithElementRef<HTMLAttributes<HTMLDivElement>> & {
		/** Short plain label, e.g. "No buckets". */
		title: string;
		action?: Snippet;
	};

	let { ref = $bindable(null), class: className, title, action, ...restProps }: Props = $props();
</script>

<div
	bind:this={ref}
	data-slot="empty-state"
	class={cn("flex flex-col items-center justify-center gap-3 px-4 py-10 text-center", className)}
	{...restProps}
>
	<p class="text-sm text-muted-foreground">{title}</p>
	{#if action}<div class="flex items-center gap-2">{@render action()}</div>{/if}
</div>
