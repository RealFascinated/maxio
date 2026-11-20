<script lang="ts">
  import { Toolbar } from '$lib/components/ui/toolbar'
  import { Input } from '$lib/components/ui/input'
  import OpsTable from './OpsTable.svelte'
  import type { MetricsView } from './metrics-view'

  let { view }: { view: MetricsView } = $props()
  let filter = $state('')
</script>

<div class="flex flex-col gap-3">
  <Toolbar>
    <Input
      type="search"
      bind:value={filter}
      placeholder="Filter operations"
      aria-label="Filter operations"
      class="sm:max-w-xs"
    />
  </Toolbar>
  <OpsTable title="Storage operations" ops={view.snapshot.storageOps} {filter} />
  <OpsTable title="Metadata operations" ops={view.snapshot.metadataOps} {filter} />
</div>
