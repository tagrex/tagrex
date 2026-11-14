<script lang="ts">
  import AnimatedSelect from './AnimatedSelect.svelte';
  let { id, value, options, label, onchange }: {
    id?: string; value: string; options: { key: string; label: string }[];
    label: string; onchange: (value: string) => void;
  } = $props();
  let native = $state<HTMLSelectElement>();
  function ownNative(element: HTMLSelectElement) {
    native = element;
    // Keep the node available while AnimatedSelect tears down its listeners.
    // The owning component releases both references when it is destroyed.
  }
</script>

<!-- Own this bridge in Svelte: the global enhancer must not reparent nodes
     whose options/conditional lifetime are controlled by this component. -->
<span class="app-select" data-native-select-owner="true">
  <select {id} {value} aria-label={label} {@attach ownNative} onchange={event => onchange(event.currentTarget.value)}>
    {#each options as item (item.key)}<option value={item.key}>{item.label}</option>{/each}
  </select>
  {#if native}<AnimatedSelect {native} />{/if}
</span>
