<script lang="ts">
  import { Tabs } from '$lib/components/ui/tabs'
  import { SECTIONS, type SectionId } from './metrics-view'

  interface Props {
    value: SectionId
    onchange: (section: SectionId) => void
  }

  let { value, onchange }: Props = $props()
</script>

<div class="overflow-x-auto [scrollbar-width:none]">
  <Tabs
    aria-label="Metrics sections"
    {value}
    items={SECTIONS.map((section) => ({ value: section.id, label: section.label }))}
    onValueChange={(next) => onchange(next as SectionId)}
  />
</div>
