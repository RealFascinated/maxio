<script lang="ts">
  import Copy from 'lucide-svelte/icons/copy'
  import Download from 'lucide-svelte/icons/download'
  import MoreHorizontal from 'lucide-svelte/icons/ellipsis'
  import Plus from 'lucide-svelte/icons/plus'
  import Trash from 'lucide-svelte/icons/trash-2'
  import List from 'lucide-svelte/icons/list'
  import LayoutGrid from 'lucide-svelte/icons/layout-grid'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { DataTableColumnHeader } from '$lib/components/ui/data-table'
  import { Dialog } from '$lib/components/ui/dialog'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { EmptyState } from '$lib/components/ui/empty-state'
  import { Helper } from '$lib/components/ui/helper'
  import { Highlighted } from '$lib/components/ui/highlighted'
  import { Input } from '$lib/components/ui/input'
  import { Kbd } from '$lib/components/ui/kbd'
  import { Label } from '$lib/components/ui/label'
  import { PageHeader } from '$lib/components/ui/page-header'
  import * as Popover from '$lib/components/ui/popover'
  import { ProgressBar } from '$lib/components/ui/progress-bar'
  import { SegmentedControl } from '$lib/components/ui/segmented-control'
  import { Select } from '$lib/components/ui/select'
  import { Skeleton } from '$lib/components/ui/skeleton'
  import { SlideOver } from '$lib/components/ui/slide-over'
  import { SplitPane } from '$lib/components/ui/split-pane'
  import { StatTile } from '$lib/components/ui/stat-tile'
  import { Switch } from '$lib/components/ui/switch'
  import * as Table from '$lib/components/ui/table'
  import { Tabs } from '$lib/components/ui/tabs'
  import { Tag } from '$lib/components/ui/tag'
  import { Toolbar } from '$lib/components/ui/toolbar'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import { appUi } from '$lib/app-ui.svelte'
  import { toast } from '$lib/toast'

  const variants = ['default', 'secondary', 'outline', 'ghost', 'destructive', 'link'] as const
  const sizes = ['xs', 'sm', 'md', 'lg'] as const

  let dialogOpen = $state(false)
  let confirmOpen = $state(false)
  let slideOverOpen = $state(false)
  let switchOn = $state(true)
  let tab = $state('objects')
  let view = $state('list')
  let selectValue = $state('us-east-1')
  let selectedRow = $state('photos')
  let sortDirection = $state<false | 'asc' | 'desc'>('asc')

  const rows = [
    { name: 'photos', objects: 18420, size: '4.2 GiB', versioning: true },
    { name: 'backups', objects: 312, size: '812.4 GiB', versioning: false },
    { name: 'logs-2026', objects: 2_104_551, size: '96.0 GiB', versioning: false },
    { name: 'static-assets', objects: 940, size: '120.3 MiB', versioning: true },
  ]

  const surfaces = [
    ['bg-background', 'background'],
    ['bg-panel', 'panel'],
    ['bg-raised', 'raised'],
    ['bg-inset', 'inset'],
  ]
</script>

{#snippet section(title: string)}
  <h2 class="mt-8 mb-2 border-b border-border pb-1 text-sm first:mt-0">{title}</h2>
{/snippet}

<div class="mx-auto flex max-w-5xl flex-col">
  <PageHeader title="Design">
    {#snippet actions()}
      <Button variant="secondary" size="sm" onclick={() => (appUi.paletteOpen = true)}>
        Command palette <Kbd>⌘K</Kbd>
      </Button>
    {/snippet}
  </PageHeader>

  {@render section('Surfaces and text')}
  <div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
    {#each surfaces as [cls, name]}
      <div class="h-16 rounded-surface border border-border-strong p-2 text-xs {cls}">{name}</div>
    {/each}
  </div>
  <div class="mt-2 flex gap-4">
    <span class="text-foreground">foreground</span>
    <span class="text-muted-foreground">muted</span>
    <span class="text-subtle">subtle</span>
    <span class="text-accent">accent</span>
    <span class="text-success">success</span>
    <span class="text-warning">warning</span>
    <span class="text-danger">danger</span>
    <span class="text-info">info</span>
  </div>
  <div class="mt-2 flex gap-1">
    {#each [1, 2, 3, 4, 5, 6, 7, 8] as n}
      <div class="h-6 flex-1 rounded-control" style:background="var(--chart-{n})" title="chart-{n}"></div>
    {/each}
  </div>

  {@render section('Type')}
  <div class="flex flex-col gap-1">
    <h1>Heading 1, 16 semibold</h1>
    <h2>Heading 2, 14 semibold</h2>
    <h3>Heading 3, 13 semibold</h3>
    <p>Body 13. The quick brown fox jumps over the lazy dog.</p>
    <p class="text-xs text-muted-foreground">Small 11, muted</p>
    <p class="font-mono text-xs">mono 11: s3://photos/2026/img_0042.jpg</p>
    <p class="num">tabular 0123456789 · 1,234,567</p>
    <p>Inline <Highlighted>highlighted</Highlighted> text and <Kbd>⌘</Kbd><Kbd>K</Kbd> keys</p>
  </div>

  {@render section('Button')}
  <div class="flex flex-col gap-2">
    {#each variants as variant}
      <div class="flex flex-wrap items-center gap-2">
        {#each sizes as size}
          <Button {variant} {size}>{variant} {size}</Button>
        {/each}
        <Button {variant} size="icon" aria-label="Add"><Plus /></Button>
        <Button {variant} size="icon-xs" aria-label="Add"><Plus /></Button>
        <Button {variant}><Download />With icon</Button>
        <Button {variant} disabled>Disabled</Button>
        <Button {variant} loading>Loading</Button>
      </div>
    {/each}
  </div>

  {@render section('Input')}
  <div class="grid max-w-xl grid-cols-2 gap-3">
    <div class="flex flex-col gap-1.5">
      <Label for="d-name">Name</Label>
      <Input id="d-name" placeholder="bucket-name" />
    </div>
    <div class="flex flex-col gap-1.5">
      <Label for="d-invalid">Invalid</Label>
      <Input id="d-invalid" value="Bad Name" aria-invalid="true" />
      <p class="text-xs text-danger">Lowercase letters, numbers and hyphens only</p>
    </div>
    <div class="flex flex-col gap-1.5">
      <Label for="d-disabled">Disabled</Label>
      <Input id="d-disabled" value="read-only" disabled />
    </div>
    <div class="flex flex-col gap-1.5">
      <Label>Region</Label>
      <Select
        bind:value={selectValue}
        aria-label="Region"
        options={[
          { value: 'us-east-1', label: 'us-east-1' },
          { value: 'eu-west-1', label: 'eu-west-1' },
          { value: 'ap-south-1', label: 'ap-south-1' },
        ]}
      />
    </div>
    <div class="flex items-center gap-2 self-end pb-1">
      <Switch checked={switchOn} aria-label="Versioning" onclick={() => (switchOn = !switchOn)} />
      <span>Versioning</span>
      <Helper text="Keep every version of an object" />
    </div>
  </div>

  {@render section('Status')}
  <div class="flex flex-wrap items-center gap-4">
    <Badge variant="success" label="Online" />
    <Badge variant="warning" label="Degraded" />
    <Badge variant="error" label="Offline" />
    <Badge variant="info" label="Syncing" />
    <Badge variant="neutral" label="Idle" />
    <Tag>read-only</Tag>
    <Tag>us-east-1</Tag>
  </div>

  {@render section('Callout')}
  <div class="flex max-w-xl flex-col gap-2">
    <Callout type="info">3 objects are pending replication.</Callout>
    <Callout type="success">Policy saved.</Callout>
    <Callout type="warning" title="Versioning suspended">New writes overwrite the current version.</Callout>
    <Callout type="danger">Request failed: 500</Callout>
  </div>

  {@render section('Toast')}
  <div class="flex gap-2">
    <Button variant="secondary" size="sm" onclick={() => toast.success('Bucket created')}>Success</Button>
    <Button variant="secondary" size="sm" onclick={() => toast.error('Request failed: 500')}>Error</Button>
    <Button variant="secondary" size="sm" onclick={() => toast.warning('Quota at 91%')}>Warning</Button>
    <Button variant="secondary" size="sm" onclick={() => toast('Copied')}>Plain</Button>
  </div>

  {@render section('Tabs and segmented control')}
  <div class="flex flex-wrap items-center justify-between gap-3">
    <Tabs
      bind:value={tab}
      aria-label="Sections"
      items={[
        { value: 'objects', label: 'Objects', count: 1204 },
        { value: 'versions', label: 'Versions' },
        { value: 'policy', label: 'Policy' },
        { value: 'settings', label: 'Settings', disabled: true },
      ]}
    />
    <SegmentedControl
      bind:value={view}
      aria-label="View"
      options={[
        { value: 'list', label: 'List', icon: List },
        { value: 'grid', label: 'Grid', icon: LayoutGrid },
      ]}
    />
  </div>

  {@render section('Toolbar and table')}
  <Toolbar class="mb-2">
    <Input class="w-56" placeholder="Filter" aria-label="Filter" />
    <Button variant="secondary" size="sm"><Plus />New bucket</Button>
    {#snippet end()}
      <span class="num text-xs text-muted-foreground">{rows.length} buckets</span>
    {/snippet}
  </Toolbar>
  <div class="max-h-56 overflow-auto rounded-surface">
    <Table.Root>
      <Table.Header>
        <Table.Row class="hover:bg-transparent">
          <Table.Head>
            <DataTableColumnHeader
              title="Name"
              header={{
                column: {
                  getCanSort: () => true,
                  getIsSorted: () => sortDirection,
                  getToggleSortingHandler: () => () => (sortDirection = sortDirection === 'asc' ? 'desc' : 'asc'),
                },
              }}
            />
          </Table.Head>
          <Table.Head class="text-right">Objects</Table.Head>
          <Table.Head class="text-right">Size</Table.Head>
          <Table.Head>Versioning</Table.Head>
          <Table.Head class="w-8"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each rows as row (row.name)}
          <Table.Row data-state={selectedRow === row.name ? 'selected' : undefined} onclick={() => (selectedRow = row.name)}>
            <Table.Cell class="font-medium">{row.name}</Table.Cell>
            <Table.Cell class="num text-right">{row.objects.toLocaleString()}</Table.Cell>
            <Table.Cell class="num text-right text-muted-foreground">{row.size}</Table.Cell>
            <Table.Cell><Badge variant={row.versioning ? 'success' : 'neutral'} label={row.versioning ? 'On' : 'Off'} /></Table.Cell>
            <Table.Cell>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger>
                  {#snippet child({ props })}
                    <Button {...props} variant="ghost" size="icon-xs" aria-label="Row actions"><MoreHorizontal /></Button>
                  {/snippet}
                </DropdownMenu.Trigger>
                <DropdownMenu.Content>
                  <DropdownMenu.Item><Copy />Copy name</DropdownMenu.Item>
                  <DropdownMenu.Item><Download />Download</DropdownMenu.Item>
                  <DropdownMenu.Separator />
                  <DropdownMenu.Item variant="destructive"><Trash />Delete</DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Root>
            </Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
  </div>

  {@render section('Overlays')}
  <div class="flex flex-wrap items-center gap-2">
    <Button variant="secondary" size="sm" onclick={() => (dialogOpen = true)}>Dialog</Button>
    <Button variant="secondary" size="sm" onclick={() => (slideOverOpen = true)}>Slide-over</Button>
    <Button variant="destructive" size="sm" onclick={() => (confirmOpen = true)}>Confirm</Button>
    <Tooltip.Root>
      <Tooltip.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" size="sm">Tooltip</Button>
        {/snippet}
      </Tooltip.Trigger>
      <Tooltip.Content>Copy to clipboard</Tooltip.Content>
    </Tooltip.Root>
    <Popover.Root>
      <Popover.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline" size="sm">Popover</Button>
        {/snippet}
      </Popover.Trigger>
      <Popover.Content class="flex flex-col gap-2">
        <Label for="d-pop">Prefix</Label>
        <Input id="d-pop" placeholder="photos/" />
      </Popover.Content>
    </Popover.Root>
  </div>

  {@render section('Tiles and progress')}
  <div class="grid grid-cols-2 gap-2 sm:grid-cols-4">
    <StatTile label="Objects" value="2,124,327" />
    <StatTile label="Stored" value="913.1" unit="GiB" />
    <StatTile label="Requests" value="1,284" unit="/s" tone="success" />
    <StatTile label="Errors" value="0.4" unit="%" tone="danger" />
  </div>
  <div class="mt-3 flex max-w-md flex-col gap-2">
    <ProgressBar value={42} />
    <ProgressBar value={78} warnAt={70} dangerAt={90} />
    <ProgressBar value={94} warnAt={70} dangerAt={90} size="md" />
  </div>

  {@render section('Card')}
  <div class="grid gap-2 sm:grid-cols-2">
    <Card>
      <CardHeader><CardTitle>Storage</CardTitle></CardHeader>
      <CardContent class="num">913.1 GiB of 2 TiB</CardContent>
    </Card>
    <Card variant="box">
      <CardHeader><CardTitle>Interactive card</CardTitle></CardHeader>
      <CardContent>Hover changes border and surface only</CardContent>
    </Card>
  </div>

  {@render section('Loading and empty')}
  <div class="grid gap-3 sm:grid-cols-2">
    <div class="flex flex-col gap-2 rounded-surface border border-border p-3">
      <Skeleton class="w-1/3" />
      <Skeleton class="w-full" />
      <Skeleton class="w-2/3" />
    </div>
    <div class="rounded-surface border border-border">
      <EmptyState title="No buckets">
        {#snippet action()}
          <Button size="sm"><Plus />Create bucket</Button>
        {/snippet}
      </EmptyState>
    </div>
  </div>

  {@render section('Split pane')}
  <SplitPane id="design-demo" class="h-32 rounded-surface border border-border" initial={200}>
    {#snippet first()}<div class="p-3">Tree</div>{/snippet}
    {#snippet second()}<div class="p-3">Detail</div>{/snippet}
  </SplitPane>
</div>

<Dialog bind:open={dialogOpen} title="Create bucket">
  <div class="flex flex-col gap-1.5">
    <Label for="d-dialog-name">Name</Label>
    <Input id="d-dialog-name" placeholder="bucket-name" />
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (dialogOpen = false)}>Cancel</Button>
    <Button onclick={() => (dialogOpen = false)}>Create</Button>
  {/snippet}
</Dialog>

<SlideOver bind:open={slideOverOpen} title="photos/img_0042.jpg">
  <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
    <dt class="text-muted-foreground">Size</dt>
    <dd class="num">2.4 MiB</dd>
    <dt class="text-muted-foreground">Type</dt>
    <dd class="font-mono text-xs">image/jpeg</dd>
    <dt class="text-muted-foreground">Modified</dt>
    <dd class="num">2026-11-18 09:41</dd>
  </dl>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (slideOverOpen = false)}>Close</Button>
    <Button><Download />Download</Button>
  {/snippet}
</SlideOver>

<ConfirmDialog
  bind:open={confirmOpen}
  title="Delete bucket photos"
  confirmLabel="Delete"
  confirmVariant="destructive"
  confirmationText="photos"
  onConfirm={() => {
    confirmOpen = false
    toast.success('Bucket deleted')
  }}
/>
