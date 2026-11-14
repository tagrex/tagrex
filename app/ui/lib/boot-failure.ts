import { createComponentElement } from './component-elements.js';

/** Keep a failed controller import recoverable without depending on its modules. */
export function showBootFailure(error: unknown) {
  document.getElementById('app')?.setAttribute('inert', '');
  const notice = document.createElement('section');
  notice.className = 'boot-failure';
  notice.setAttribute('role', 'alert');
  const heading = document.createElement('h1');
  heading.textContent = 'TagRex couldn’t start';
  const message = document.createElement('p');
  message.textContent = 'Reload the app to try again.';
  const reload = createComponentElement('button', { variant: 'default' });
  reload.textContent = 'Reload';
  reload.addEventListener('click', () => location.reload());
  const details = document.createElement('details');
  const summary = document.createElement('summary');
  summary.textContent = 'Error details';
  const reason = document.createElement('pre');
  reason.textContent = error instanceof Error ? error.message : String(error);
  details.append(summary, reason);
  notice.append(heading, message, reload, details);
  document.body.append(notice);
  reload.focus();
}
