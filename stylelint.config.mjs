// Root stylelint config (F3). Component CSS (`*.module.css`) carries no raw colour and no px:
// every colour and pixel value comes from @shadowrunr/tokens custom properties. Generated token
// CSS and global base styles are outside this gate (tokens.css is checked by F1's drift gate).
export default {
  // The legacy desk's global CSS is deleted with it at cutover C2.
  ignoreFiles: ["**/node_modules/**", "**/dist/**", "packages/tokens/src/tokens.css", "apps/console/src/legacy/**"],
  rules: {
    "block-no-empty": true,
    "declaration-block-no-duplicate-properties": true,
    "property-no-unknown": true,
  },
  overrides: [
    {
      files: ["**/*.module.css"],
      rules: {
        "color-no-hex": true,
        "color-named": "never",
        "function-disallowed-list": ["rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch", "color"],
        "unit-disallowed-list": ["px"],
      },
    },
  ],
};
