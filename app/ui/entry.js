import './fonts.css';
import './shadcn.css';
import './style.css';
import './workbench.css';
import './components/selects.css';
import './premium.css';
import './surfaces.css';
import './spreadsheet.css';
import './table.css';
import './workbench-docking.css';
import './form-controls.css';
import './preset-editor.css';
import './workbench-system.css';
import './palettes.css';
import './inspector.css';
import './js/interface-fonts.js';
import { flushSync, mount } from 'svelte';
import App from './components/App.svelte';
import { initComponentControls } from './lib/runtime-controls';
import { initArtworkWheel } from './lib/artwork-wheel';
import { initControllerSurfaces } from './lib/controller-surfaces';
import { showBootFailure } from './lib/boot-failure';

// Existing workflow controllers attach to stable DOM IDs at module evaluation.
// Render the entire Svelte view before loading them, including hidden dialogs.
flushSync(() => mount(App, { target: document.getElementById('app') }));
initComponentControls();
initArtworkWheel(document);
initControllerSurfaces();
import('./app.js').catch(showBootFailure);
