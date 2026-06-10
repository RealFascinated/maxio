<script lang="ts">
  import { page } from '$app/state'
  import BucketSettings from '$lib/components/BucketSettings.svelte'

  const bucket = $derived(decodeURIComponent(page.params.bucket!))
</script>

<BucketSettings {bucket} />
