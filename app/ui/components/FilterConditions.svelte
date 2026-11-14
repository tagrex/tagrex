<script lang="ts">
  import * as Popover from '$lib/components/ui/popover/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import FilterSelect from './FilterSelect.svelte';
  import { Input } from '$lib/components/ui/input/index.js';
  import { FILTER_FIELDS, FILTER_OPERATORS, conditionLabel } from '../js/filter-query.js';

  type Condition = { id: string; field: string; operator: string; value: string };
  let conditions = $state<Condition[]>([]);
  let mode = $state('all');
  let open = $state(false);
  let fieldSearch = $state('');
  let field = $state('cover');
  let operator = $state('equals');
  let value = $state('missing');
  let editing = $state<string | null>(null);
  let sequence = 0;
  let error = $state('');
  let pending = $state(false);
  let hidden = $state(0);
  let matched = $state(0);
  let total = $state(0);
  let valid = $derived(['empty', 'present'].includes(operator) || Boolean(value.trim()));
  let fields = $derived(FILTER_FIELDS.filter(item => item.label.toLowerCase().includes(fieldSearch.toLowerCase())));

  function receive() {
    const listener = (event: Event) => {
      const state = (event as CustomEvent).detail;
      conditions = state.conditions;
      mode = state.mode;
      error = state.error;
      pending = state.pending;
      hidden = state.hidden;
      matched = state.matched;
      total = state.total;
    };
    document.addEventListener('tagrex:filter-state', listener);
    document.dispatchEvent(new Event('tagrex:request-filter-state'));
    return () => document.removeEventListener('tagrex:filter-state', listener);
  }
  function publish(next: Condition[], combination = mode) {
    document.dispatchEvent(new CustomEvent('tagrex:filter-conditions', { detail: { conditions: next, mode: combination } }));
  }
  function chooseField(next: string) {
    field = next;
    operator = next === 'cover' ? 'equals' : 'contains';
    value = next === 'cover' ? 'missing' : '';
  }
  function edit(item: Condition) {
    editing = item.id;
    fieldSearch = '';
    field = item.field; operator = item.operator; value = item.value;
    open = true;
  }
  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!valid) return;
    const item = { id: editing || `condition-${Date.now()}-${++sequence}`, field, operator, value };
    publish(editing ? conditions.map(old => old.id === editing ? item : old) : [...conditions, item]);
    open = false;
    editing = null;
  }
</script>

<div class="filter-condition-controls" {@attach receive}>
  <Popover.Root bind:open onOpenChange={next => {
    if (!next) editing = null;
    else if (!editing) { fieldSearch = ''; chooseField('cover'); }
  }}>
    <Popover.Trigger>
      {#snippet child({ props })}
        <Button {...props} id="filter-add" variant="ghost" size="sm">Add filter</Button>
      {/snippet}
    </Popover.Trigger>
    <Popover.Content side="top" align="end" class="filter-builder" aria-label={editing ? 'Edit filter' : 'Add filter'}>
      <form onsubmit={submit}>
        <label for="filter-field-search">Find a field</label>
        <Input id="filter-field-search" bind:value={fieldSearch} oninput={event => {
          const text = event.currentTarget.value.toLowerCase();
          const matches = FILTER_FIELDS.filter(item => item.label.toLowerCase().includes(text));
          if (matches.length && !matches.some(item => item.key === field)) chooseField(matches[0].key);
        }} placeholder="Artist, artwork…" />
        <label for="filter-field">Field</label>
        <FilterSelect id="filter-field" value={field} options={fields} label="Field" onchange={chooseField} />
        {#if !fields.length}<span class="filter-hint">No matching fields.</span>{/if}
        {#if field !== 'cover'}
          <label for="filter-operator">Match</label>
          <FilterSelect id="filter-operator" value={operator} options={FILTER_OPERATORS} label="Match" onchange={next => { operator = next; }} />
        {/if}
        {#if field === 'cover'}
          <label for="filter-value">Artwork status</label>
          <FilterSelect id="filter-value" {value} label="Artwork status" options={[{ key: 'missing', label: 'Missing' }, { key: 'embedded', label: 'Embedded' }, { key: 'unknown', label: 'Unknown' }]} onchange={next => { value = next; }} />
        {:else if !['empty', 'present'].includes(operator)}
          <label for="filter-value">Value</label>
          <Input id="filter-value" bind:value maxlength={512} />
        {/if}
        <div class="filter-builder-actions">
          <Button variant="ghost" type="button" onclick={() => { open = false; editing = null; }}>Cancel</Button>
          <Button type="submit" disabled={!valid || !fields.length || (!editing && conditions.length >= 24)}>{editing ? 'Update filter' : 'Add condition'}</Button>
        </div>
      </form>
    </Popover.Content>
  </Popover.Root>
</div>
{#if conditions.length}
  <div class="filter-condition-list" aria-label="Filter conditions">
    {#if conditions.length > 1}
      <label class="filter-combination">Match
        <FilterSelect value={mode} label="Combine filter conditions" options={[{ key: 'all', label: 'all' }, { key: 'any', label: 'any' }]} onchange={next => publish(conditions, next)} />
      </label>
    {/if}
    {#each conditions as item (item.id)}
      <span class="filter-condition-chip">
        <Button variant="ghost" size="sm" onclick={() => edit(item)} aria-label={`Edit ${conditionLabel(item)}`}>{conditionLabel(item)}</Button>
        <Button variant="ghost" size="sm" aria-label={`Remove ${conditionLabel(item)}`} onclick={() => {
          publish(conditions.filter(old => old.id !== item.id));
          requestAnimationFrame(() => document.getElementById('filter-add')?.focus());
        }}>×</Button>
      </span>
    {/each}
  </div>
{/if}
<div id="filter-feedback" class={['filter-feedback', { 'filter-error': !!error }]} role="status" aria-live="polite">
  {#if error}{error}
  {:else if pending}Updating results…
  {:else if total && !matched}No files match. Edit or clear a filter.
  {:else if total}<span class="sr-only">{matched} of {total} files match.</span>
  {/if}
  {#if hidden && !pending}<span>{hidden} selected {hidden === 1 ? 'file is' : 'files are'} hidden by the current view.</span>{/if}
</div>
