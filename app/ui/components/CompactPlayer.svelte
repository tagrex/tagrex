<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
</script>

<!-- Purpose-built slots keep the generic control enhancer from replacing these native ranges. -->
<div id="player" class="idle" role="group" aria-label="Audio player">
  <div class="pl-transport">
    <Button variant="ghost" size="icon-sm" id="pl-prev" class="icon" disabled data-i18n-title="player.prev" title="Previous track" data-i18n-aria-label="player.prev" aria-label="Previous track"><svg class="ico" aria-hidden="true"><use href="#i-prev"/></svg></Button>
    <Button variant="ghost" size="icon-sm" id="pl-toggle" class="icon" disabled data-i18n-title="player.toggle" title="Play / pause" data-i18n-aria-label="player.toggle" aria-label="Play / pause"><svg class="ico" aria-hidden="true"><use href="#i-play"/></svg></Button>
    <Button variant="ghost" size="icon-sm" id="pl-stop" class="icon" disabled data-i18n-title="player.stop" title="Stop" data-i18n-aria-label="player.stop" aria-label="Stop"><svg class="ico" aria-hidden="true"><use href="#i-stop"/></svg></Button>
    <Button variant="ghost" size="icon-sm" id="pl-next" class="icon" disabled data-i18n-title="player.next" title="Next track" data-i18n-aria-label="player.next" aria-label="Next track"><svg class="ico" aria-hidden="true"><use href="#i-next"/></svg></Button>
    <Button variant="ghost" size="icon-sm" id="pl-repeat" class="icon" data-i18n-title="player.repeatOff" title="Repeat off" data-i18n-aria-label="player.repeatOff" aria-label="Repeat off"><svg class="ico" aria-hidden="true"><use href="#i-repeat"/></svg></Button>
  </div>
  <div class="pl-main">
    <div class="pl-line">
      <button type="button" id="pl-title" data-slot="player-title" class="mono" disabled data-i18n="player.noTrack">No track loaded</button>
      <span id="pl-time" class="muted">0:00 / 0:00</span>
    </div>
    <input id="pl-seek" data-slot="player-range" type="range" min="0" max="1000" value="0" step="1" disabled data-i18n-aria-label="player.seek" aria-label="Seek" aria-valuetext="0:00 / 0:00" />
  </div>
  <div class="pl-vol">
    <Button variant="ghost" size="icon-sm" id="pl-mute" class="icon" data-i18n-title="player.mute" title="Mute" data-i18n-aria-label="player.mute" aria-label="Mute"><svg class="ico" aria-hidden="true"><use href="#i-volume"/></svg></Button>
    <input id="pl-volume" data-slot="player-range" type="range" min="0" max="100" value="100" step="1" data-i18n-title="player.volume" title="Volume" data-i18n-aria-label="player.volume" aria-label="Volume" />
  </div>
</div>
