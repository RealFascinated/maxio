import {
  rowSortingFeature,
  sortFns,
  tableFeatures,
} from '@tanstack/svelte-table'

/// Sorting feature without a client row model — for server-sorted tables.
export const serverSortableTableFeatures = tableFeatures({
  rowSortingFeature,
  sortFns,
})
