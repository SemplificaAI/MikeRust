<!-- Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only. -->
<!-- Generic searchable single/multi-select picker (documents, workflows…). -->
<script lang="ts" module>
  export interface PickerItem {
    id: string
    label: string
    sublabel?: string
    /** Optional filter key (e.g. primary domain) matched against the filter select. */
    tag?: string
    /** Extra filter keys — the item matches the filter if `tag` OR any of
     *  these equals the selected value (e.g. a template's also-applicable
     *  domains). */
    tags?: string[]
  }
</script>

<script lang="ts">
  import Modal from './Modal.svelte'
  import Input from './Input.svelte'
  import Select from './Select.svelte'
  import Button from './Button.svelte'
  import Spinner from './Spinner.svelte'
  import EmptyState from './EmptyState.svelte'
  import { i18n } from '$lib/stores/i18n.svelte'
  import { Search, Check } from 'lucide-svelte'

  interface Props {
    open?: boolean
    title: string
    items: PickerItem[]
    multi?: boolean
    loading?: boolean
    /** Pre-selected ids. */
    initial?: string[]
    /** When set, renders a filter select that matches `PickerItem.tag`. */
    filterOptions?: { value: string; label: string }[]
    /** Current filter value (bindable). Empty string = no filter. */
    filterValue?: string
    onpick: (ids: string[]) => void
  }

  let {
    open = $bindable(false),
    title,
    items,
    multi = false,
    loading = false,
    initial = [],
    filterOptions,
    filterValue = $bindable(''),
    onpick,
  }: Props = $props()

  const t = (k: string) => i18n.t(k)

  let query = $state('')
  let selected = $state<Set<string>>(new Set())

  $effect(() => {
    if (open) {
      query = ''
      selected = new Set(initial)
    }
  })

  const filtered = $derived(
    items.filter((i) => {
      if (!i.label.toLowerCase().includes(query.trim().toLowerCase())) return false
      if (filterValue && i.tag !== filterValue && !(i.tags ?? []).includes(filterValue))
        return false
      return true
    })
  )

  function toggle(id: string) {
    if (multi) {
      const next = new Set(selected)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      selected = next
    } else {
      selected = new Set([id])
    }
  }

  function confirm() {
    onpick([...selected])
    open = false
  }
</script>

<Modal bind:open {title} size="md">
  <div class="space-y-3">
    <div class="flex items-center gap-2">
      <Input bind:value={query} placeholder={t('Common.search')} size="sm" class="flex-1">
        {#snippet iconBefore()}
          <Search size={14} />
        {/snippet}
      </Input>
      {#if filterOptions}
        <Select options={filterOptions} bind:value={filterValue} size="sm" class="w-40" />
      {/if}
    </div>

    <div class="h-72 overflow-y-auto -mx-1 px-1">
      {#if loading}
        <div class="flex items-center justify-center gap-2 text-sm text-(--color-text-secondary) py-12">
          <Spinner size="sm" />
          {t('Common.loading')}
        </div>
      {:else if filtered.length === 0}
        <EmptyState title={t('Common.none')} />
      {:else}
        <ul class="flex flex-col gap-1">
          {#each filtered as item (item.id)}
            {@const on = selected.has(item.id)}
            <li>
              <button
                type="button"
                onclick={() => toggle(item.id)}
                class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-(--radius-md) text-left
                       transition-colors duration-(--transition-fast)
                       focus:outline-none focus-visible:ring-2 focus-visible:ring-(--color-brand-500)
                       {on ? 'bg-(--color-brand-50)' : 'hover:bg-(--color-hover-bg)'}"
              >
                <span
                  class="shrink-0 inline-flex h-4 w-4 items-center justify-center rounded-(--radius-sm) border
                         {on
                           ? 'bg-(--color-brand-500) border-(--color-brand-500) text-white'
                           : 'border-(--color-surface-300)'}"
                >
                  {#if on}<Check size={11} />{/if}
                </span>
                <span class="flex-1 min-w-0">
                  <span class="block text-sm text-(--color-text-primary) truncate">{item.label}</span>
                  {#if item.sublabel}
                    <span class="block text-xs text-(--color-text-secondary) truncate">{item.sublabel}</span>
                  {/if}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{t('Common.cancel')}</Button>
    <Button disabled={selected.size === 0} onclick={confirm}>{t('Common.confirm')}</Button>
  {/snippet}
</Modal>
