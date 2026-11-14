# Desktop development

Install Node.js 22.12 or newer, Rust, and the platform dependencies required by
[Tauri](https://v2.tauri.app/start/prerequisites/), then run:

```sh
npm ci
npm run desktop:dev
```

`app/.taurignore` keeps frontend saves out of the Rust watcher.

The desktop shell loads Vite at `http://127.0.0.1:1420`. CSS saves update the
open window live. Svelte component and controller JavaScript saves refresh the
frontend so controllers rebind to the new DOM. Rust saves rebuild and restart
the desktop shell. Saved view preferences restore after a frontend refresh;
transient drafts are not persisted. Component HMR is disabled while controllers
retain direct element references.
The port is fixed and fails if already occupied, so the shell cannot silently
connect to the wrong development server.

`app/tauri.dev.conf.json` contains the development URL and a localhost-only
WebSocket allowance. The normal build configuration and packaged CSP remain
unchanged. Use `npm run build` for bundled frontend assets.
