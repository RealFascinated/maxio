# MaxIO Console Design System

Dense, keyboard-first, neutral. Closer to Linear or Grafana than to a marketing site. Dark by default, full light parity. Tokens live in `ui/src/app.css`; primitives live in `ui/src/lib/components/ui/*`; `/design` renders every primitive in every state.

## Principles

1. Data first. Real rows, real numbers, real labels. Chrome stays out of the way.
2. One accent. Violet marks the primary action, focus, active tab and selection. Nothing else is coloured unless it carries status.
3. Flat. Surfaces separate with a 1px hairline, not with shadows, gradients or tint.
4. Terse. Plain sentence-case labels. Messages state facts.
5. Keyboard parity. Everything reachable by mouse is reachable by keyboard; `⌘K` reaches every page.

## Anti-patterns

Do not add any of these. Reviewers should reject them.

- Subtitles or description lines under page titles or section headings. A title stands alone. `PageHeader` has no description prop.
- Explanatory helper paragraphs, onboarding copy, marketing-style empty states. An empty state is one short label plus an optional action (`No buckets`, `Create bucket`).
- Chatty or apologetic copy: "Oops", "Something went wrong", "Welcome back", "Let's get started", exclamation marks, em-dash prose. Write `Request failed: 500`, `Bucket created`, `Session expired`.
- Emoji anywhere.
- Gradients, glows, coloured shadows, decorative drop shadows, glass or `backdrop-blur`. Only floating layers (menu, popover, dialog, slide-over) carry `shadow-pop`.
- Coloured left-border stripes on alerts, toasts, callouts or cards. Use a flat neutral surface plus a small status dot or a coloured icon/text.
- Tinted background for everything. `*-soft` tokens are for rare, small highlights, never for whole alerts, cards or buttons.
- Pills or any third radius. `rounded-full` is only for avatars, status dots and switch thumbs. Everything else is `rounded-control` or `rounded-surface`.
- Uppercase, letter-spaced micro-labels (`uppercase tracking-wide`). Labels are sentence case.
- Decorative icons next to every heading, in empty states, or inside coloured squares/tiles. Icons exist for actions, navigation and status only.
- Card inside card. One level of bordered surface per region.
- Hover lift, scale or bounce animations. Hover changes colour only.
- Accent colour for decoration. If removing the colour loses no meaning, remove it.
- Multi-colour icons, illustrations, hero banners.
- Native browser controls and popups: `<select>`/`<option>`, `title=` tooltips, native date/colour pickers, visible file inputs, number spinners. Use `Select`, `Tooltip`, and a `Button` that triggers a hidden file input.

## Tokens

CSS variables switch under `.dark` on `<html>`. Tailwind utilities are mapped in `@theme inline`.

### Surfaces and borders

| Token | Tailwind | Dark | Light | Use |
|---|---|---|---|---|
| `--bg` | `bg-background` | `#0d0e10` | `#f4f5f7` | App background |
| `--panel` | `bg-panel`, `bg-card` | `#131417` | `#ffffff` | Top bar, rail, tables, cards |
| `--raised` | `bg-raised`, `bg-popover` | `#1a1b1f` | `#ffffff` | Menus, popovers, dialogs, toasts, secondary buttons |
| `--inset` | `bg-inset`, `bg-input` | `#0f1012` | `#f6f7f9` | Input fill, dialog footer, kbd |
| `--hover` | `bg-hover`, `bg-muted` | white 4.5% | ink 5% | Row/item hover |
| `--selected` | `bg-selected` | white 8% | ink 8% | Selected row, active nav item |
| `--border` | `border-border` | `#25272c` | `#e2e4e9` | Hairlines (default border colour) |
| `--border-strong` | `border-border-strong` | `#34373e` | `#cbd0d8` | Input and button borders, floating layers |

Elevation: `bg` < `panel` < `raised`. A surface is separated from its parent by a hairline, never by a shadow.

### Text

| Token | Tailwind | Use |
|---|---|---|
| `--fg` | `text-foreground` | Primary text |
| `--muted` | `text-muted-foreground` | Secondary text, labels, column headers |
| `--subtle` | `text-subtle` | Placeholders, hints, disabled-ish text |

### Accent and semantic

| Token | Dark | Light | Notes |
|---|---|---|---|
| `--accent` | `#7c5cff` | `#5b3df5` | `bg-accent`, `text-accent`, `ring`. White text on it (`text-accent-foreground`) |
| `--accent-hover` | `#917aff` | `#4a2ee0` | `hover:bg-accent-hover` |
| `--accent-soft` | 16% accent | 10% accent | Text selection only |
| `--success` `--warning` `--danger` `--info` | `#3dd68c` `#f5a524` `#f4636a` `#5aaeff` | `#17835a` `#a86200` `#d1343b` `#1f6fd1` | Status dots, status text, icons. `destructive`/`error` alias `danger` |
| `--*-soft` | 13% tint | 10% tint | Rare small highlights only |

### Chart palette

`--chart-1..8` (`bg-chart-1`, `var(--chart-3)`): sky, amber, green, yellow, indigo, orange, pink, grey. Okabe-Ito derived, colourblind-safe, tuned per theme. Use in order; never rely on colour alone, add direct labels or distinct line styles.

### Shape, type, spacing

| Item | Value |
|---|---|
| Radius | Exactly two: `rounded-control` 4px, `rounded-surface` 6px (see Radius rule) |
| Control height | md 28px (`h-7`), xs/sm 24px (`h-6`), lg 32px (`h-8`) |
| Table row | 32px (`h-row`, `h-8`) |
| Top bar | 40px. Rail 48px (208px expanded) |
| Font | Geist Sans; Geist Mono for keys, IDs, paths, code |
| Spacing | 4px grid. Use `1 2 3 4 6 8` (4–32px). Page padding 16px, section gap 24px (`mt-6`), control gap 8px |

Radius rule. Two tokens, nothing else:

- `rounded-control` (`--radius-control`, 4px): buttons, inputs, select triggers, tags, kbd, tabs, segmented control, menu and list items, nav items, skeletons, progress bars, switch.
- `rounded-surface` (`--radius-surface`, 6px): cards, tables, stat tiles, callouts, dialogs, slide-over/drawer edges, popovers, menus, select lists, tooltips, toasts, the palette.

A nested element is never rounder than its parent: controls inside a surface use `rounded-control`. Tailwind's scale is remapped in `@theme` (`rounded-xs/sm/md` = 4px, `rounded-lg/xl/2xl/3xl` = 6px) so legacy classes land on one of the two values; new code uses the named tokens.

Type scale (Tailwind steps remapped in `app.css`):

| Step | Size / line | Use |
|---|---|---|
| `text-xs` | 11/16 | Table meta, kbd, captions, counts |
| `text-sm` (base) | 13/18 | Body, controls, table cells |
| `text-base` | 14/20 | h2 |
| `text-lg` | 16/22 | h1 page title |
| `text-xl` | 18/24 | Stat values |
| `text-2xl` | 22/28 | Rare |

Weights: 400 body, 500 labels and buttons, 600 headings. Headings use `-0.01em` tracking. Numbers in tables, stats and timestamps use the `num` utility (tabular figures) and right-align.

## Density rules

- Default control is 28px. Use `xs` (24px) inside tables, toolbars and dense forms; `lg` (32px) only for single primary actions such as login.
- Table rows 32px, no zebra, hairline row separators, header sticky with `text-xs` muted text.
- Padding: panels `p-3`, dialog/slide-over body `p-4`, page `p-4`. Never `p-6+`.
- One font size per row. Secondary data goes in `text-muted-foreground`, not a smaller size.
- Prefer inline columns to stacked label/value blocks. Show units next to values.

## Component catalog

Import from `$lib/components/ui/<folder>`. Prop APIs of existing primitives are stable.

| Component | Import | Notes |
|---|---|---|
| `Button` | `button` | `variant`: `default` (accent), `secondary`, `outline`, `ghost`, `destructive`, `link`. `size`: `xs`, `sm`, `md`, `lg`, `icon`, `icon-xs`, `icon-sm`, `icon-lg`. `loading` shows a spinner and disables. `href` renders an anchor. `highlighted` and `brand` are deprecated aliases of `default` |
| `Input` | `input` | Bordered, inset fill, accent border and ring on focus, `aria-invalid` turns the border red |
| `Select` | `select` | `options: {value,label}[]`, `bind:value`, `size` |
| `Switch` | `switch` | `checked` plus your own `onclick` |
| `Label` | `label` | 11px muted |
| `Badge` | `badge` | Status dot, optional `label`. Variants `success warning error info neutral` |
| `Tag` | `tag` | Static neutral chip for metadata (region, `read-only`) |
| `Kbd` | `kbd` | Single key cap |
| `Callout` | `callout` | Inline message. Flat panel, status dot or `icon` snippet. Types `info success warning danger`. One sentence |
| `Card` | `card` | `variant="box"` for clickable. Subparts `CardHeader/Title/Content/Footer/Action` |
| `Table` | `table` | `Table.Root/Header/Head/Body/Row/Cell/Footer`. `data-state="selected"` on a row selects it |
| `DataTableColumnHeader` | `data-table` | Sort toggle for TanStack columns |
| `Dialog` | `dialog` | bits-ui dialog: focus trap, scroll lock, Esc. `title`, `footer` snippet, `size`, `loading` |
| `ConfirmDialog` | `confirm-dialog` | `confirmationText` forces typing the resource name for destructive actions |
| `SlideOver` | `slide-over` | Right panel for object details and editors, 448px |
| `Tabs` | `tabs` | Underline. `items` with optional `href` so the active tab lives in the URL; `bind:value` otherwise |
| `SegmentedControl` | `segmented-control` | 2–4 mutually exclusive view options, icon or text |
| `DropdownMenu` | `dropdown-menu` | `Root Trigger Content Item RadioGroup RadioItem Group Label Separator`. `Item variant="destructive"`, `shortcut` snippet |
| `Tooltip` | `tooltip` | `Root Trigger Content`. Wrap app in `Tooltip.Provider` (done in the shell). Icon-only buttons need `aria-label` and a tooltip |
| `Popover` | `popover` | Small forms and filters |
| `Helper` | `helper` | Tooltip-backed `?` icon. Use sparingly; prefer clearer labels |
| `Skeleton` | `skeleton` | Flat pulse. Match the final row height |
| `EmptyState` | `empty-state` | `title` (short label) and optional `action` snippet. No icon, no body copy |
| `PageHeader` | `page-header` | `title`, `breadcrumbs` and `actions` snippets. No description |
| `Toolbar` | `toolbar` | Filter row. Children left, `end` snippet right |
| `StatTile` | `stat-tile` | `label`, `value`, `unit`, `tone`, optional `chart` snippet |
| `ProgressBar` | `progress-bar` | 0–100, `warnAt`/`dangerAt` switch colour |
| `SplitPane` | `split-pane` | Resizable two-pane, width persisted per `id`; arrow keys resize |
| `CommandPalette` | `command-palette` | Generic palette over `CommandItem[]`; the app wires it in `AppCommands.svelte` |
| `Highlighted` | `highlighted` | Medium-weight inline emphasis |
| `Sonner` | `sonner` | Toasts via `toast` from `$lib/toast`. Flat, hairline, status colour on the icon only |

Utilities: `num` (tabular numbers), `h-row`, `shadow-pop`. `cn()` from `$lib/utils`. Fuzzy scoring in `$lib/fuzzy`.

## Layout patterns

Page area is `p-4` inside the shell and scrolls on its own; nothing renders under the rail.

**List page** (buckets, users, policies): `PageHeader` (title, actions: primary button) → `Toolbar` (filter input left, count right) → `Table` in a bordered panel. Row click or `⌘K` opens detail. Empty: `EmptyState` inside the table area. Row actions in a `DropdownMenu` on an `icon-xs` ghost button; destructive ones via `ConfirmDialog`.

**Detail page** (bucket, object browser, settings): top bar breadcrumbs carry location; `PageHeader` title, then `Tabs` (URL-backed) → content. Object details open in a `SlideOver`, or a `SplitPane` (tree/list left, detail right) on wide screens. Settings forms are single-column, label above input, max width `max-w-xl`, one `Save` button right-aligned, no section descriptions.

**Dashboard** (metrics): row of `StatTile`s (`grid-cols-2 lg:grid-cols-4`, `gap-2`), then chart panels in a 12-col grid, each a single bordered panel with a 13px title and a unit in the axis. Time-range `SegmentedControl` and refresh in the `PageHeader` actions. Series use `--chart-N` in order.

**Forms in dialogs**: `Dialog` with fields, footer has `Cancel` (secondary) and the action (default). Enter submits. Disabled primary until valid; show the error under the field in `text-danger text-xs`.

## App shell

- Rail: 48px icon rail, tooltips on hover, expands to 208px (`[` toggles, persisted in `localStorage['sidebar-collapsed']`). Opaque `bg-panel`, one right hairline, never overlays content. Items 32px: Buckets, Users, Policies, Metrics, Settings (footer). Active item is a neutral `bg-selected` fill with foreground text. Root-only items hide for non-root users.
- Mobile (<1024px): rail hidden; the menu button opens an off-canvas left drawer with a backdrop.
- Top bar: 40px. Route-derived breadcrumbs (`components/app/topbar/breadcrumbs.ts`), search button (`⌘K`), theme toggle, account menu (theme choice, keyboard shortcuts, sign out).
- Theme: `light | system | dark`, stored in `localStorage['theme']`, default dark.

## Command palette and shortcuts

`⌘K` / `Ctrl+K` or `/` opens the palette. Fuzzy match (`$lib/fuzzy`) over navigation, actions and bucket names (TanStack `bucketKeys.list()`). Recent selections persist and show first on an empty query. `↑ ↓` (or `Ctrl+N/P`) move, `Enter` runs, `Esc` closes.

`$lib/shortcuts.ts`: `register(combos, handler, { description, group })` returns an unregister function. Combos: `'mod+k'`, `'/'`, `'?'`, chords as space-separated steps (`'g b'`). Plain-key shortcuts are ignored while typing in an input or while a dialog or menu is open; `mod+` combos always fire. Described shortcuts appear in the `?` help dialog automatically.

| Keys | Action |
|---|---|
| `⌘K`, `/` | Command palette |
| `?` | Shortcut help |
| `[` | Toggle sidebar |
| `g b` `g u` `g p` `g m` `g s` | Go to Buckets, Users, Policies, Metrics, Settings |

The palette's "Create bucket" navigates to `/?new`; the buckets page opens its create dialog when `new` is present in the query.

## Motion

- Colour/background transitions only on hover and focus: 100–150ms, no transform.
- Overlays: fade 120ms; dialog and menus a 2px scale-in; slide-over 16px slide; drawer 16px slide. Keyframes `ui-fade-in`, `ui-pop-in`, `ui-slide-in-right`, `ui-slide-in-left` in `app.css`.
- Skeletons use a plain opacity pulse.
- `prefers-reduced-motion: reduce` collapses all animation and transition durations globally.

## Accessibility

- Contrast: body text and controls meet WCAG AA in both themes; white on accent passes AA for 13px medium text.
- Focus: 2px accent outline with 2px offset on every focusable element via `:focus-visible` (inputs 0 offset). Never remove it; do not hide focus on custom controls.
- Icon-only buttons have `aria-label`; tooltips supplement, not replace, labels.
- Dialogs, menus, popovers, selects and tooltips come from bits-ui (roles, focus trap, Esc, arrow keys). Do not hand-roll overlays.
- Status is never colour only: pair dots and tints with a text label.
- Tabs use `role="tablist"`, arrow-key navigation; sortable headers are buttons.
- Touch targets stay 28px minimum in tables on desktop; on mobile give actions `size="md"` or larger.
- Respect `prefers-color-scheme` only when theme is `system`.

## Frontend conventions

- Svelte 5 runes, TanStack Query for server state, Tailwind v4, bun.
- Every `fetch` catch logs via `console.error` with context (`'fetchBuckets failed:'`).
- Use tokens and the primitives. Do not introduce raw hex colours, `neutral-*`/`gray-*` palette classes, or bespoke buttons and inputs. The legacy aliases `coollabs-*`, `coolgray-*`, `base` and `warning-50..900` exist only so unmigrated pages stay readable; remove them from `app.css` once nothing references them.
