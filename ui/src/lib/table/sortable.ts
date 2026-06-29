import {
  createSortedRowModel,
  renderComponent,
  rowSortingFeature,
  sortFns,
  tableFeatures,
  type HeaderContext,
  type RowData,
} from '@tanstack/svelte-table'
import DataTableColumnHeader from '$lib/components/ui/data-table/column-header.svelte'

export const sortableTableFeatures = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns,
})

export function sortableHeader(title: string, className?: string) {
  return ((ctx: HeaderContext<typeof sortableTableFeatures, RowData>) =>
    renderComponent(DataTableColumnHeader, { header: ctx.header, title, class: className })) as never
}
