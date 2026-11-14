<script lang="ts">
  let { node, home }: { node: HTMLElement; home: HTMLElement } = $props();
  function retain(host: HTMLElement) {
    host.append(node);
    return { destroy() { home.append(node); } };
  }
</script>

<div class="controller-toast-host" use:retain></div>
