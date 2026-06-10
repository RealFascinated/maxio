<script lang="ts">
  import { page } from '$app/state'
  import ObjectBrowser from '$lib/components/ObjectBrowser.svelte'
  import { pathToPrefix } from '$lib/navigation'

  const bucket = $derived(decodeURIComponent(page.params.bucket!))
  const prefix = $derived(pathToPrefix(page.params.prefix))
</script>

<ObjectBrowser {bucket} {prefix} />
