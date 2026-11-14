// Undeclared-name check for the frontend modules (#764).
//
// The interface in app/ui is plain ES modules with no build step, so a name a
// module uses but neither declares nor imports only fails when that line runs,
// and not at all inside a try/catch. That hid #763 for months. This config
// turns on that one rule and nothing else: style stays out of scope.
import { fileURLToPath } from "node:url";
import globals from "globals";

// A relative basePath that leaves this folder matches nothing, so anchor the
// patterns at the repository root by absolute path.
const repoRoot = fileURLToPath(new URL("../..", import.meta.url));

export default [
  {
    basePath: repoRoot,
    files: ["app/ui/**/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      // Everything else the modules reach (the Tauri bridge, the mock) goes
      // through `window.`, so the standard browser globals are the whole list.
      globals: globals.browser,
    },
    rules: {
      "no-undef": "error",
    },
  },
];
