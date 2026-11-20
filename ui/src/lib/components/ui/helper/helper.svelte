<script lang="ts">
	import { cn } from "$lib/utils.js";
	import HelpCircle from "lucide-svelte/icons/circle-help";
	import * as Tooltip from "$lib/components/ui/tooltip";

	type Props = {
		text: string;
		class?: string;
	};

	let { text, class: className }: Props = $props();
</script>

<Tooltip.Root>
	<Tooltip.Trigger
		class={cn("inline-flex items-center text-subtle transition-colors hover:text-foreground", className)}
		aria-label={text}
	>
		<HelpCircle class="size-3.5" />
	</Tooltip.Trigger>
	<Tooltip.Content class="max-w-xs">{text}</Tooltip.Content>
</Tooltip.Root>
