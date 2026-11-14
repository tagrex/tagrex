// Runs the undeclared-name check over app/ui (#764).
//
// Before trusting a clean result it lints a canary that uses an undeclared
// name, as if it sat in app/ui. If the config ever stops matching those files
// (a moved folder, a changed basePath), ESLint falls back to no rules and
// reports nothing, which would read as a pass. The canary makes that a failure.
import { fileURLToPath } from "node:url";
import { ESLint } from "eslint";

const here = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const eslint = new ESLint({ cwd: repoRoot, overrideConfigFile: `${here}eslint.config.js` });

const [canary] = await eslint.lintText("export const value = undeclaredCanaryName;\n", {
  filePath: `${repoRoot}app/ui/js/__lint_canary__.js`,
});
if (!canary.messages.some((m) => m.ruleId === "no-undef")) {
  console.error("lint config does not apply to app/ui: the canary's undeclared name went unreported");
  process.exit(2);
}

const results = await eslint.lintFiles(["app/ui"]);
const formatter = await eslint.loadFormatter("stylish");
const output = await formatter.format(results);
if (output) console.log(output);
const problems = results.reduce((n, r) => n + r.errorCount + r.warningCount, 0);
console.log(`checked ${results.length} files in app/ui, ${problems} problem(s)`);
process.exit(problems ? 1 : 0);
