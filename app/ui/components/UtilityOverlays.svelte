<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect, NativeSelectOption } from '$lib/components/ui/native-select/index.js';
  import ControllerTooltip from './ControllerTooltip.svelte';
  import { Toaster } from '$lib/components/ui/sonner/index.js';
  import ControllerDialog from './ControllerDialog.svelte';
  import ControllerPopover from './ControllerPopover.svelte';
  import ControllerContextMenu from './ControllerContextMenu.svelte';
  import ControllerMenuItem from './ControllerMenuItem.svelte';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
</script>

    <div id="toast" class="toast" hidden role="status"><div class="toast-body"><p id="toast-title" class="toast-title"></p><ul id="toast-parts" class="toast-parts" hidden></ul></div><Button variant="ghost" size="icon-sm" id="toast-close" class="icon toast-close" type="button" hidden data-i18n-title="action.close" title="Close" data-i18n-aria-label="action.close" aria-label="Close"><svg class="ico"><use href="#i-close"/></svg></Button></div>

    <!-- Viewport-positioned query choices stay outside inspector containment. -->
    <div id="query-preset-menu" class="col-menu" hidden></div>

    <ControllerTooltip />
    <Toaster position="bottom-center" visibleToasts={1} offset="var(--controller-toast-bottom, 20px)" mobileOffset="16px" class="controller-toaster" />

    <div id="drop-overlay" aria-hidden="true">
      <div class="drop-cue">
        <div class="drop-cue-title" data-i18n="drop.title">Drop here</div>
        <div class="drop-cue-sub" data-i18n="drop.sub">A folder or files open · a single image becomes the cover</div>
      </div>
    </div>

    <ControllerContextMenu id="edit-ctx">
      <ControllerMenuItem cmd="cut"><svg class="ico"><use href="#i-cut"/></svg><span data-i18n="ctx.cut">Cut</span><span class="ctx-key" data-combo="Mod+KeyX"></span></ControllerMenuItem>
      <ControllerMenuItem cmd="copy"><svg class="ico"><use href="#i-copy"/></svg><span data-i18n="ctx.copy">Copy</span><span class="ctx-key" data-combo="Mod+KeyC"></span></ControllerMenuItem>
      <ControllerMenuItem cmd="paste"><svg class="ico"><use href="#i-paste"/></svg><span data-i18n="ctx.paste">Paste</span><span class="ctx-key" data-combo="Mod+KeyV"></span></ControllerMenuItem>
      <ContextMenu.Separator class="ctx-sep" />
      <ControllerMenuItem cmd="selectall"><svg class="ico"><use href="#i-select-all"/></svg><span data-i18n="ctx.selectAll">Select all</span><span class="ctx-key" data-combo="Mod+KeyA"></span></ControllerMenuItem>
    </ControllerContextMenu>

    <ControllerContextMenu id="file-ctx">
      <ControllerMenuItem cmd="bpm"><svg class="ico"><use href="#i-metronome"/></svg><span data-i18n="toolbar.detectBpm">Detect BPM</span><span class="ctx-key" data-action="detectBpm"></span></ControllerMenuItem>
      <ControllerMenuItem cmd="clear"><svg class="ico"><use href="#i-eraser"/></svg><span data-i18n="editor.clearTags">Clear tags</span><span class="ctx-key" data-action="clearTags"></span></ControllerMenuItem>
      <ControllerMenuItem cmd="formats"><svg class="ico"><use href="#i-tag"/></svg><span data-i18n="staging.tagFormats">Tag formats…</span></ControllerMenuItem>
      <ContextMenu.Separator class="ctx-sep" />
      <ControllerMenuItem cmd="hide"><svg class="ico"><use href="#i-list-remove"/></svg><span class="ctx-label" data-i18n="ctx.removeFromList">Remove from the list</span></ControllerMenuItem>
      <ContextMenu.Separator class="ctx-sep" />
      <ControllerMenuItem cmd="trash" danger><svg class="ico"><use href="#i-trash"/></svg><span class="ctx-label" data-i18n="ctx.trash">Move to Trash…</span><span class="ctx-key" data-action="trash"></span></ControllerMenuItem>
    </ControllerContextMenu>

    <ControllerDialog id="tag-formats-modal" labelledby="tag-formats-title" closeId="tag-formats-close">
      <div class="modal tag-formats-modal">
        <div class="modal-head"><h2 id="tag-formats-title" data-i18n="staging.tagFormats">Tag formats…</h2><Button id="tag-formats-close" variant="ghost" size="icon-sm" class="icon" aria-label="Close"><svg class="ico"><use href="#i-close"/></svg></Button></div>
        <p id="fields-blocks" class="hint muted" hidden></p>
        <div id="fields-blocks-convert" class="blocks-convert" hidden></div>
      </div>
    </ControllerDialog>

    <ControllerDialog id="colmask-modal" labelledby="colmask-title" closeId="colmask-close">
      <div class="modal colmask">
        <div class="modal-head">
          <h2 id="colmask-title">Add column</h2>
          <Button variant="ghost" size="icon-sm" id="colmask-close" class="icon" type="button" data-i18n-title="action.cancel" title="Cancel" data-i18n-aria-label="action.cancel" aria-label="Cancel"><svg class="ico"><use href="#i-close"/></svg></Button>
        </div>
        <div class="colmask-fields">
          <label class="colmask-row">
            <span data-i18n="colmask.name">Name</span>
            <Input id="colmask-name" type="text" spellcheck="false" data-i18n-placeholder="colmask.namePlaceholder" placeholder="Artist · Title"  />
          </label>
          <label class="colmask-row">
            <span data-i18n="colmask.pattern">Pattern</span>
            <div class="colmask-pattern field">
              <Input id="colmask-pattern" type="text" class="mono" spellcheck="false" placeholder="%artist% - %title%"  />
              <div class="field-tools">
                <Button variant="ghost" size="icon-sm" class="icon ph-btn" type="button" data-ph-target="colmask-pattern" data-i18n-title="mask.placeholders" title="Placeholders" data-i18n-aria-label="mask.placeholdersAria" aria-label="Placeholder reference"><svg class="ico"><use href="#i-help"/></svg></Button>
              </div>
            </div>
          </label>
          <div class="colmask-row">
            <span data-i18n="colmask.align">Align</span>
            <div id="colmask-align" class="seg">
              <Button variant="ghost" size="sm" type="button" class="seg-btn active" data-align="left" data-i18n="colmask.left">Left</Button>
              <Button variant="ghost" size="sm" type="button" class="seg-btn" data-align="center" data-i18n="colmask.center">Center</Button>
              <Button variant="ghost" size="sm" type="button" class="seg-btn" data-align="right" data-i18n="colmask.right">Right</Button>
            </div>
          </div>
        </div>
        <p class="settings-hint" data-i18n="colmask.hint">Read-only. The value is computed, so the cell cannot be edited. Sorting uses the displayed text, and values reflect what is on disk. Staged edits appear after Apply.</p>
        <div class="modal-actions">
          <Button variant="outline" size="sm" id="colmask-cancel" data-i18n="action.cancel">Cancel</Button>
          <Button variant="default" size="sm" id="colmask-ok" class="primary">Add column</Button>
        </div>
      </div>
    </ControllerDialog>

    <ControllerDialog id="mask-builder" labelledby="mb-title" closeId="mb-close">
      <div class="modal mask-builder">
        <div class="modal-head">
          <h2 id="mb-title">Mask builder</h2>
          <Button variant="ghost" size="icon-sm" id="mb-close" class="icon" type="button" data-i18n-title="action.cancel" title="Cancel" data-i18n-aria-label="action.cancel" aria-label="Cancel"><svg class="ico"><use href="#i-close"/></svg></Button>
        </div>
        <div class="mb-body">
          <div class="mb-main">
            <div class="field multi mb-field">
              <Textarea id="mb-expr" rows={1} spellcheck="false" autocomplete="off" data-i18n-aria-label="maskBuilder.expression" aria-label="Mask"></Textarea>
              <div class="field-tools">
                <Button variant="ghost" size="icon-sm" id="mb-clear" class="icon field-clear" type="button" data-i18n-title="maskBuilder.clear" title="Clear the mask" data-i18n-aria-label="maskBuilder.clear" aria-label="Clear the mask"><svg class="ico"><use href="#i-close"/></svg></Button>
                <span class="field-sep" aria-hidden="true"></span>
                <Button variant="ghost" size="icon-sm" class="icon preset-btn" type="button" data-preset-target="mb-expr" data-i18n-title="preset.title" title="Mask presets" data-i18n-aria-label="preset.title" aria-label="Mask presets"><svg class="ico"><use href="#i-presets"/></svg></Button>
                <Button variant="ghost" size="icon-sm" id="mb-transform" class="icon" type="button" title="Transform values" aria-label="Transform values"><svg class="ico"><use href="#i-wand"/></svg><span id="mb-rules" class="field-count"></span></Button>
              </div>
            </div>
            <div class="mb-status">
              <p id="mb-error" class="mb-error" role="alert" hidden></p>
              <p id="mb-summary" class="mb-summary" aria-live="polite"></p>
            </div>
            <div id="mb-list" class="mb-list"></div>
          </div>
          <div class="mb-ref">
            <div class="field is-empty mb-ref-search">
              <Input id="mb-ref-filter" type="search" data-i18n-placeholder="ph.filter" placeholder="Filter placeholders…" spellcheck="false" autocomplete="off"  />
              <div class="field-tools">
                <Button variant="ghost" size="icon-sm" id="mb-ref-clear" class="icon field-clear" type="button" data-i18n-title="ph.clearFilter" title="Clear the filter" data-i18n-aria-label="ph.clearFilter" aria-label="Clear the filter"><svg class="ico"><use href="#i-close"/></svg></Button>
              </div>
            </div>
            <div id="mb-ref-body" class="ph-body mb-ref-body"></div>
            <p class="ph-foot muted"><span data-i18n="ph.foot">Click one to insert it at the caret.</span><Button variant="link" size="sm" class="ph-guide" data-i18n="ph.guide">Mask guide</Button></p>
          </div>
        </div>
        <div class="modal-actions">
          <Button variant="outline" size="sm" id="mb-cancel" type="button" data-i18n="action.cancel">Cancel</Button>
          <Button variant="default" size="sm" id="mb-ok" class="primary" type="button" data-i18n="maskBuilder.ok">Use</Button>
        </div>
      </div>
    </ControllerDialog>

    <ControllerPopover id="ph-menu" class="ph-menu" label="Placeholders" labelKey="mask.placeholders">
      <div class="ph-head">
        <div class="field sm is-empty">
          <Input id="ph-filter" type="search" data-i18n-placeholder="ph.filter" placeholder="Filter placeholders…" spellcheck="false" autocomplete="off"  />
          <div class="field-tools">
            <Button variant="ghost" size="icon-sm" id="ph-filter-clear" class="icon field-clear" type="button" data-i18n-title="ph.clearFilter" title="Clear the filter" data-i18n-aria-label="ph.clearFilter" aria-label="Clear the filter"><svg class="ico"><use href="#i-close"/></svg></Button>
          </div>
        </div>
      </div>
      <div id="ph-body" class="ph-body"></div>
      <p class="ph-foot muted"><span data-i18n="ph.foot">Click one to insert it at the caret.</span><Button variant="link" size="sm" class="ph-guide" data-i18n="ph.guide">Mask guide</Button></p>
    </ControllerPopover>

    <ControllerPopover id="preset-menu" class="ph-menu preset-menu" label="Mask presets" labelKey="preset.title">
      <div class="ph-head preset-save-row">
        <Input id="preset-name" type="text" data-i18n-placeholder="preset.namePlaceholder" placeholder="Name this mask" spellcheck="false" autocomplete="off"  />
        <Button variant="outline" size="sm" id="preset-save" class="btn" type="button" data-i18n="preset.save">Save</Button>
      </div>
      <div id="preset-list" class="ph-body preset-list"></div>
      <p class="ph-foot muted" data-i18n="preset.foot">Click a preset to use it in this field.</p>
    </ControllerPopover>

    <ControllerDialog id="transform-modal" labelledby="transform-modal-title" closeId="transform-modal-close">
      <div class="modal transform-modal">
        <div class="modal-head">
          <h2 id="transform-modal-title" data-i18n="generator.transform">Transform values</h2>
          <Button variant="ghost" size="icon-sm" id="transform-modal-close" class="icon" type="button" data-i18n-title="action.done" title="Done" data-i18n-aria-label="action.done" aria-label="Done"><svg class="ico"><use href="#i-close"/></svg></Button>
        </div>
        <div id="transform-modal-body" class="transform-modal-body"></div>
      </div>
    </ControllerDialog>

    <ControllerDialog id="pe-editor" labelledby="pe-editor-title" closeId="pe-editor-close" describedby="pe-editor-purpose">
      <div class="modal pe-editor">
        <div class="modal-head">
          <div class="pe-heading-line"><h2 id="pe-editor-title" data-i18n="presets.title">Transformation presets</h2><span class="pe-save-note" data-i18n="presets.saveNote">Saves automatically · Files change on Apply</span></div>
          <Button variant="ghost" size="icon-sm" id="pe-editor-close" class="icon" type="button" data-i18n-title="action.done" title="Done" data-i18n-aria-label="action.done" aria-label="Done"><svg class="ico"><use href="#i-close"/></svg></Button>
        </div>
        <p id="pe-editor-purpose" class="pe-intro" data-i18n="presets.purpose">Create reusable rules to tidy tags and file names.</p>
        <div class="pe-editor-body">
          <div class="pe-list-col">
            <div id="pe-list" class="pe-list"></div>
            <div class="pe-list-actions"><Button variant="default" size="sm" id="pe-new" class="btn primary" type="button"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg><span data-i18n="presets.newAction">New preset</span></Button></div>
            <div id="pe-deleted" class="pe-deleted" hidden role="status"><span id="pe-deleted-label"></span><Button variant="ghost" size="sm" id="pe-undo-delete" type="button" data-i18n="action.undo">Undo</Button></div>
          </div>
          <div id="pe-edit-col" class="pe-edit-col">
            <section class="pe-sample" aria-labelledby="pe-preview-title">
              <div class="pe-preview-head">
                <h3 id="pe-preview-title" data-i18n="presets.previewTitle">Preview</h3>
                <div class="pe-preview-actions">
                  <label class="pe-preview-target"><span data-i18n="presets.fieldLabel">Field</span><select id="pe-preview-field" aria-label="Preview field" data-i18n-aria-label="presets.previewField"></select></label>
                  <Button variant="ghost" size="sm" class="text-btn" type="button" id="pe-selection" data-i18n="presets.selection">Selection</Button>
                  <Button variant="ghost" size="sm" class="text-btn" type="button" id="pe-example" data-i18n="presets.example">Example</Button>
                </div>
              </div>
              <label class="pe-sample-row"><span class="pe-sample-label" data-i18n="presets.sample">Before</span><Input id="pe-sample-input" type="text" spellcheck="false" autocomplete="off" /></label>
              <div class="pe-result"><span class="pe-sample-label" data-i18n="presets.result">After</span><div id="pe-sample-out" class="pe-sample-out" role="textbox" tabindex="0" aria-readonly="true" aria-label="Preview result" data-i18n-aria-label="presets.previewResult" aria-live="polite"></div></div>
              <span id="pe-preview-status" class="pe-sr-only" role="status"></span>
            </section>
            <section class="pe-rules-panel" aria-labelledby="pe-rules-title">
              <div class="pe-rules-heading"><div class="pe-rules-title-group"><h3 id="pe-rules-title" data-i18n="presets.rulesTitle">Rules</h3><span id="pe-rules-scope" class="pe-rules-scope" hidden></span></div><Button id="pe-copy-to-edit" variant="ghost" size="sm" type="button" class="text-btn" hidden data-i18n="presets.copyToEdit">Copy to edit</Button></div>
              <fieldset id="pe-rules-set" class="pe-rules-set" aria-labelledby="pe-rules-title" aria-describedby="pe-rules-scope">
                <div id="pe-rules" class="rule-chain"></div>
                <div id="pe-empty" class="pe-empty-state" hidden><p data-i18n="presets.emptyTitle">No rules yet.</p><Button id="pe-empty-add" variant="default" size="sm" class="primary" type="button"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg><span data-i18n="presets.addRule">Add rule</span></Button></div>
                <div class="pe-add-row"><Button id="pe-open-add" variant="ghost" size="sm" class="text-btn" type="button" aria-haspopup="menu" aria-controls="pe-rule-picker"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg><span data-i18n="presets.addRule">Add rule</span></Button></div>
              </fieldset>

            </section>
          </div>
        </div>
      </div>
        <ControllerContextMenu id="pe-preset-menu"><ControllerMenuItem cmd="rename"><svg class="ico" aria-hidden="true"><use href="#i-rename"/></svg><span data-i18n="presets.rename">Rename</span></ControllerMenuItem><ControllerMenuItem cmd="copy"><svg class="ico"><use href="#i-copy"/></svg><span data-i18n="presets.duplicate">Copy</span></ControllerMenuItem><ContextMenu.Separator class="ctx-sep" /><ControllerMenuItem cmd="delete" danger><svg class="ico"><use href="#i-trash"/></svg><span data-i18n="action.delete">Delete</span></ControllerMenuItem></ControllerContextMenu>
        <ControllerContextMenu id="pe-rule-picker"><ControllerMenuItem cmd="replace"><span><span data-i18n="rule.replace">Find and replace</span><small data-i18n="presets.kind.replace.hint">Replace or remove text.</small></span></ControllerMenuItem><ControllerMenuItem cmd="case"><span><span data-i18n="rule.case">Change case</span><small data-i18n="presets.kind.case.hint">Change capital letters.</small></span></ControllerMenuItem><ControllerMenuItem cmd="diacritics"><span><span data-i18n="rule.diacritics">Remove accents</span><small data-i18n="presets.kind.diacritics.hint">Turn é into e.</small></span></ControllerMenuItem><ControllerMenuItem cmd="transliterate"><span><span data-i18n="rule.transliterate">Use Latin letters</span><small data-i18n="presets.kind.transliterate.hint">Convert other writing systems.</small></span></ControllerMenuItem><ControllerMenuItem cmd="untransliterate"><span><span data-i18n="rule.untransliterate">Use Cyrillic letters</span><small data-i18n="presets.kind.untransliterate.hint">Convert Latin text to Cyrillic.</small></span></ControllerMenuItem><ControllerMenuItem cmd="key"><span><span data-i18n="rule.key">Musical key format</span><small data-i18n="presets.kind.key.hint">Switch between key notations.</small></span></ControllerMenuItem></ControllerContextMenu>
    </ControllerDialog>

    <ControllerDialog role="alertdialog" id="confirm-modal" labelledby="confirm-message" closeId="confirm-cancel" describedby="confirm-detail">
      <div class="modal confirm">
        <p id="confirm-message" class="modal-msg"></p>
        <p id="confirm-detail" class="modal-detail" hidden></p>
        <div class="modal-actions">
          <Button variant="outline" size="sm" id="confirm-cancel" data-i18n="action.cancel">Cancel</Button>
          <Button variant="default" size="sm" id="confirm-ok" class="primary" data-i18n="action.ok">OK</Button>
        </div>
      </div>
    </ControllerDialog>
