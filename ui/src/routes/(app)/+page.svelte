<script lang="ts">
  import { createQuery } from '@tanstack/svelte-query'
  import BucketList from '$lib/components/BucketList.svelte'
  import { checkAuth } from '$lib/api/auth'
  import { authKeys } from '$lib/api/keys'

  const authQuery = createQuery(() => ({
    queryKey: authKeys.check(),
    queryFn: checkAuth,
    retry: false,
  }))

  const isRootUser = $derived(authQuery.data?.isRoot === true)
  const canCreateBucket = $derived(
    isRootUser || authQuery.data?.capabilities?.canCreateBucket === true,
  )
</script>

<BucketList {canCreateBucket} />
