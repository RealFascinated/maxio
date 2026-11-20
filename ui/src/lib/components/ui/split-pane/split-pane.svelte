<script lang="ts">
	import { cn } from "$lib/utils.js";
	import type { Snippet } from "svelte";

	type Props = {
		/** localStorage key; the first pane width is persisted under `split-pane:<id>`. */
		id: string;
		/** Initial first-pane width in px. */
		initial?: number;
		min?: number;
		max?: number;
		class?: string;
		first: Snippet;
		second: Snippet;
	};

	let { id, initial = 280, min = 160, max = 640, class: className, first, second }: Props = $props();

	const storageKey = $derived(`split-pane:${id}`);
	let stored = $state<number | null>(null);
	const width = $derived(stored ?? initial);
	let dragging = $state(false);
	let container = $state<HTMLDivElement | null>(null);

	$effect(() => {
		const saved = Number(localStorage.getItem(storageKey));
		if (saved) stored = clamp(saved);
	});

	function clamp(value: number) {
		const limit = container ? Math.min(max, container.clientWidth - min) : max;
		return Math.min(Math.max(value, min), limit);
	}

	function persist() {
		localStorage.setItem(storageKey, String(Math.round(width)));
	}

	function startDrag(event: PointerEvent) {
		dragging = true;
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function drag(event: PointerEvent) {
		if (!dragging || !container) return;
		stored = clamp(event.clientX - container.getBoundingClientRect().left);
	}

	function endDrag() {
		if (!dragging) return;
		dragging = false;
		persist();
	}

	function handleKeydown(event: KeyboardEvent) {
		const step = event.shiftKey ? 48 : 16;
		if (event.key === "ArrowLeft") stored = clamp(width - step);
		else if (event.key === "ArrowRight") stored = clamp(width + step);
		else return;
		event.preventDefault();
		persist();
	}
</script>

<div bind:this={container} data-slot="split-pane" class={cn("flex min-h-0 min-w-0", className)}>
	<div class="min-h-0 shrink-0 overflow-auto" style:width="{width}px">{@render first()}</div>
	<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
	<div
		role="separator"
		aria-orientation="vertical"
		aria-valuenow={Math.round(width)}
		aria-valuemin={min}
		aria-valuemax={max}
		tabindex="0"
		class={cn(
			"group relative w-px shrink-0 cursor-col-resize touch-none bg-border outline-none after:absolute after:inset-y-0 after:-left-1 after:w-[9px] hover:bg-accent focus-visible:bg-accent",
			dragging && "bg-accent"
		)}
		onpointerdown={startDrag}
		onpointermove={drag}
		onpointerup={endDrag}
		onpointercancel={endDrag}
		onkeydown={handleKeydown}
	></div>
	<div class="min-h-0 min-w-0 flex-1 overflow-auto">{@render second()}</div>
</div>
