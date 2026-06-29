<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
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

<div class="space-y-2">
  <Label>Principal</Label>
  <div class="flex flex-wrap gap-2">
    <Button
      type="button"
      variant={mode === 'everyone' ? 'highlighted' : 'outline'}
      size="sm"
      disabled={readonly}
      onclick={setEveryone}
    >
      Everyone (*)
    </Button>
    <Button
      type="button"
      variant={mode === 'custom' ? 'highlighted' : 'outline'}
      size="sm"
      disabled={readonly}
      onclick={setCustom}
    >
      AWS principals
    </Button>
  </div>
  {#if mode === 'custom'}
    <Input
      type="text"
      placeholder="arn:aws:iam::maxio:user/alice, comma-separated"
      class="font-mono text-xs"
      value={customArns}
      readonly={readonly}
      oninput={(e) => updateCustomArns(e.currentTarget.value)}
    />
  {/if}
</div>
