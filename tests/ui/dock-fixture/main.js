import { mount } from 'svelte';
import '../../../app/ui/shadcn.css';
import '../../../app/ui/style.css';
import '../../../app/ui/workbench.css';
import DockFixture from './DockFixture.svelte';
mount(DockFixture, { target: document.getElementById('dock-fixture-root') });
