<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip/index.js';
  import { connectControllerSurface, registerControllerTooltip } from '$lib/controller-surfaces.js';
  let open = $state(false);
  let anchor = $state<HTMLElement | null>(null);
  let text = $state('');
  let hint = $state('');
  let root: HTMLElement;
  function connect(node: HTMLElement) {
    root = node;
    const bridge = connectControllerSurface(root, (value) => { open = value; });
    const unregister = registerControllerTooltip((target, label, shortcut) => {
      anchor = target;
      text = label;
      hint = shortcut;
      root.hidden = false;
    });
    return { destroy() { unregister(); bridge.destroy(); } };
  }
  function changed(value: boolean) { if (!value && open) root.hidden = true; }
</script>

<div id="tooltip" class="controller-tooltip-root" hidden use:connect>
  <Tooltip.Provider delayDuration={0}>
    <Tooltip.Root {open} onOpenChange={changed} disableHoverableContent>
      <Tooltip.Content forceMount portalProps={{ disabled: true }} customAnchor={anchor} side="bottom" sideOffset={6}
        class="controller-tooltip-content" arrowClasses="hidden" id="controller-tooltip-content">
        {#if text}<span>{text}</span>{/if}
        {#if hint}<kbd class="tip-kbd">{hint}</kbd>{/if}
      </Tooltip.Content>
    </Tooltip.Root>
  </Tooltip.Provider>
</div>
