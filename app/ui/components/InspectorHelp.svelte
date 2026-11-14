<script lang="ts">
  import * as Popover from '$lib/components/ui/popover/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { t } from '../js/i18n.js';

  let { labelKey, helpKey }: { labelKey: string; helpKey: string } = $props();
</script>

<Popover.Root>
  <Popover.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" class="inspector-help" data-i18n-aria-label={labelKey} aria-label={t(labelKey)}>
        <svg class="ico" aria-hidden="true"><use href="#i-info" /></svg>
      </Button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content class="inspector-help-content" align="start" collisionPadding={12}>
    <p data-i18n={helpKey}>{t(helpKey)}</p>
  </Popover.Content>
</Popover.Root>
