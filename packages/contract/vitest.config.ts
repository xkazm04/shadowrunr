// Coverage excludes src/generated/**: it is emitted from the Rust schema (crates/shadowrunr-types)
// and validated by its own generator check (pnpm run check), so it is not hand-written behaviour.
// A ratchet over it would measure ajv output and hide the hand-written door.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    coverage: {
      exclude: ['src/generated/**', '**/*.test.*'],
    },
  },
});
