<script lang="ts">
  import { Input } from '$lib/components/ui/input'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import type { PrincipalSpec } from '$lib/policy/types'

  interface Props {
    principal: PrincipalSpec | undefined
    readonly?: boolean
    onchange?: (principal: PrincipalSpec | undefined) => void
  }

  let { principal = $bindable(undefined), readonly = false, onchange }: Props = $props()

  const mode = $derived(
    principal === '*'
      ? 'everyone'
      : principal && typeof principal === 'object'
        ? 'custom'
        : 'everyone',
  )

  let customArns = $state('')

  $effect(() => {
    if (principal && typeof principal === 'object') {
      const aws = principal.AWS
      customArns = Array.isArray(aws) ? aws.join(', ') : aws
    } else if (principal === undefined) {
      customArns = ''
    }
  })

  function setEveryone() {
    if (readonly) return
    principal = '*'
    onchange?.(principal)
  }

  function setCustom() {
    if (readonly) return
    principal = { AWS: '' }
    onchange?.(principal)
  }

  function updateCustomArns(value: string) {
    customArns = value
    if (readonly) return
    const parts = value
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)
    principal = parts.length === 1 ? { AWS: parts[0] } : { AWS: parts }
    onchange?.(principal)
  }
</script>

<div class="flex min-w-0 flex-col gap-2">
  <SegmentedControl
    class="self-start"
    aria-label="Principal"
    value={mode}
    options={[
      { value: 'everyone', label: 'Everyone (*)', disabled: readonly },
      { value: 'custom', label: 'AWS principals', disabled: readonly },
    ]}
    onValueChange={(next) => (next === 'everyone' ? setEveryone() : setCustom())}
  />
  {#if mode === 'custom'}
    <Input
      type="text"
      placeholder="arn:aws:iam::maxio:user/alice, comma-separated"
      class="font-mono text-xs"
      aria-label="AWS principals"
      value={customArns}
      readonly={readonly}
      oninput={(e) => updateCustomArns(e.currentTarget.value)}
    />
  {/if}
</div>
