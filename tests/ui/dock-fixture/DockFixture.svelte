<script lang="ts">
  import { onMount } from 'svelte';
  import DockWorkspace from '../../../app/ui/components/DockWorkspace.svelte';
  import DockModule from '../../../app/ui/components/DockModule.svelte';
  import { Input } from '../../../app/ui/lib/components/ui/input/index.js';
  import { Button } from '../../../app/ui/lib/components/ui/button/index.js';
  import type { WorkspaceLayout } from '../../../app/ui/lib/workspace-layout';
  let controller = $state<WorkspaceLayout>();
  let text = $state('A title in progress');
  let clicks = $state(0);
  onMount(() => {
    const original = document.getElementById('dock-edit');
    Object.assign(window, { dockTest: { original, get controller() { return controller; } } });
  });
</script>
<div class="fixture-workspace">
  <DockWorkspace storageKey="tagrex.dock-test.v1" bind:controller>
    <DockModule moduleId="files" title="Files" defaultArea="left" chromeTarget=".fixture-files-header">
      <div class="fixture-files-header">Files toolbar</div>
      <Input id="dock-edit" bind:value={text} aria-label="Persistent title" />
      <output id="dock-value">{text}</output>
      <Button id="dock-legacy" onclick={() => clicks++}>Existing action</Button>
      <output id="dock-clicks">{clicks}</output>
    </DockModule>
    <DockModule moduleId="editor" title="Editor" defaultArea="right"><p>Tag editor</p></DockModule>
    <DockModule moduleId="online" title="Online" defaultArea="right"><p>Online results</p></DockModule>
    <DockModule moduleId="transforms" title="Transformations" defaultArea="top"><p>Transformation presets</p></DockModule>
  </DockWorkspace>
</div>
<style>
  :global(html), :global(body), :global(#dock-fixture-root) { width:100%; height:100%; margin:0; }
  .fixture-workspace { width:100%; height:100dvh; display:flex; }
  .fixture-workspace :global(.fixture-files-header) { display:flex; align-items:center; min-height:28px; flex:none; }
  .fixture-workspace :global(.dock-module-body) { padding:4px; gap:4px; }
</style>
