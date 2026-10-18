// Root eslint config (F3). Gates, all at zero and each proven red once on a seeded violation:
//   boundaries   apps/console/src layers import one way only (boundaries/dependencies)
//   cycles       no import cycles (import-x/no-cycle)
//   hygiene      no `any`, no `as unknown as`, no JSON.parse outside @shadowrunr/contract's door,
//                no raw keydown listener outside src/keys/listener.ts, no empty (swallowed) catch
//
// Typed linting is off on purpose: the workspace compiles with TypeScript 7 (no JS API), so the
// parser runs on the root TypeScript 6 devDependency without type information.
import js from "@eslint/js";
import boundaries from "eslint-plugin-boundaries";
import { createTypeScriptImportResolver } from "eslint-import-resolver-typescript";
import importX from "eslint-plugin-import-x";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const C = "apps/console/src";
const ROOT = dirname(fileURLToPath(import.meta.url));
const CONSOLE_TSCONFIG = join(ROOT, "apps/console/tsconfig.json");

/** The pre-port desk: reachable at #/legacy until cutover C2 deletes all of it. */
const LEGACY_DIRS = [`${C}/legacy/**`, `${C}/views/**`, `${C}/api/**`, `${C}/lib/**`, `${C}/fixtures/**`];
const LEGACY = [...LEGACY_DIRS, `${C}/contract.ts`];

/** The one door: JSON.parse is allowed here and nowhere else. */
const DOOR = ["packages/contract/src/**"];

const restricted = (allowJsonParse, allowKeydown) => [
  "error",
  {
    selector: "TSAsExpression[expression.type='TSAsExpression'][expression.typeAnnotation.type='TSUnknownKeyword']",
    message: "No `as unknown as`: validate at the door or narrow with a type guard.",
  },
  ...(allowJsonParse
    ? []
    : [
        {
          selector: "CallExpression[callee.object.name='JSON'][callee.property.name='parse']",
          message: "JSON.parse lives only in @shadowrunr/contract's door (parseRpcResult / parseEvent).",
        },
      ]),
  ...(allowKeydown
    ? []
    : [
        {
          selector: "CallExpression[callee.property.name='addEventListener'][arguments.0.value=/^key(down|up|press)$/]",
          message: "No raw keyboard listeners: declare the binding in src/keys/keymap.ts.",
        },
        {
          selector: "AssignmentExpression[left.property.name=/^onkey(down|up|press)$/]",
          message: "No raw keyboard listeners: declare the binding in src/keys/keymap.ts.",
        },
      ]),
];

/** A boundaries policy: elements of type `from` may import only elements of the `to` types. */
const allow = (from, to) => ({
  from: { element: { type: from } },
  allow: [{ to: { element: { types: { anyOf: to } } } }],
});

export default tseslint.config(
  {
    ignores: [
      "**/node_modules/**",
      "**/dist/**",
      "**/target/**",
      "**/src-tauri/**",
      "**/generated/**",
      "runs/**",
      ".contest/**",
      "apps/stage/**",
      ...LEGACY,
    ],
  },
  {
    files: ["apps/console/**/*.{ts,tsx}", "packages/*/src/**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    languageOptions: { parser: tseslint.parser, ecmaVersion: 2022, sourceType: "module" },
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unused-vars": ["error", { caughtErrors: "all", argsIgnorePattern: "^_" }],
      "no-empty": ["error", { allowEmptyCatch: false }],
      "no-restricted-syntax": restricted(false, false),
      "no-restricted-properties": ["error", { object: "JSON", property: "parse", message: "JSON.parse lives only in the contract door." }],
    },
  },
  {
    // Shared packages' React components (stage-kit's art and ui) get the hooks rules too.
    files: ["packages/*/src/**/*.tsx"],
    plugins: { "react-hooks": reactHooks },
    rules: reactHooks.configs.flat.recommended.rules,
  },
  {
    files: DOOR,
    rules: { "no-restricted-syntax": restricted(true, false), "no-restricted-properties": "off" },
  },
  {
    files: [`${C}/keys/listener.ts`],
    rules: { "no-restricted-syntax": restricted(false, true) },
  },
  {
    files: [`${C}/**/*.{ts,tsx}`],
    plugins: { "react-hooks": reactHooks, boundaries, "import-x": importX },
    settings: {
      "import/resolver": { typescript: { project: CONSOLE_TSCONFIG } },
      "import-x/resolver-next": [createTypeScriptImportResolver({ project: CONSOLE_TSCONFIG })],
      "import-x/extensions": [".ts", ".tsx", ".js"],
      "import-x/parsers": { "@typescript-eslint/parser": [".ts", ".tsx"] },
      "boundaries/root-path": ROOT,
      "boundaries/include": [`${C}/**/*`],
      "boundaries/elements": [
        { type: "app", pattern: `${C}/app/**`, partialMatch: false },
        { type: "shell", pattern: `${C}/shell/**`, partialMatch: false },
        { type: "kit", pattern: `${C}/kit/**`, partialMatch: false },
        { type: "page", pattern: `${C}/pages/*/**`, partialMatch: false, capture: ["page"] },
        { type: "router", pattern: `${C}/router/**`, partialMatch: false },
        { type: "keys", pattern: `${C}/keys/**`, partialMatch: false },
        { type: "data", pattern: `${C}/data/**`, partialMatch: false },
        { type: "ui", pattern: `${C}/ui/**`, partialMatch: false },
        { type: "legacy", pattern: LEGACY_DIRS, partialMatch: false },
      ],
    },
    rules: {
      ...reactHooks.configs.flat.recommended.rules,
      // Advisory for the React Compiler, which this app does not run (TanStack Virtual trips it).
      "react-hooks/incompatible-library": "off",
      "import-x/no-cycle": ["error", { ignoreExternal: true }],
      "boundaries/no-unknown-files": "error",
      "boundaries/dependencies": [
        "error",
        {
          default: "disallow",
          // An import of a file outside every element (src/contract.ts, say) is checked too.
          checkUnknownLocals: true,
          message: "{{from.type}} may not import {{to.type}} (see eslint.config.js boundaries).",
          policies: [
            allow("app", ["shell", "kit", "page", "router", "keys", "data", "ui", "legacy"]),
            allow("shell", ["kit", "router", "keys", "data", "ui"]),
            allow("page", ["kit", "router", "data", "ui"]),
            // Files of one page import each other across its subfolders (the captured page id
            // must match); one page never imports another.
            {
              from: { element: { type: "page" } },
              allow: [{ to: { element: { type: "page", captured: { page: "{{ from.element.captured.page }}" } } } }],
            },
            allow("kit", ["router", "data", "ui"]),
            allow("legacy", ["legacy"]),
          ],
        },
      ],
    },
  },
);
