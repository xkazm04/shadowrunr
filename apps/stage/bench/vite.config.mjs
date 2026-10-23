// The bench page's production build (`vite build --config bench/vite.config.mjs --outDir <dir>`):
// the stage's root, with the bench page as the only entry, so a CPU or ready-time run loads the
// faces as a production stage would (minified, `import.meta.env.DEV` false, each face its own chunk).
// The dev server (frames, sheet, clips) runs on it too. Both serve the page cross-origin isolated,
// so performance.now() ticks in microseconds instead of Chrome's coarsened 100 us: a face's CPU
// time per frame is a fraction of a millisecond.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const stage = join(dirname(fileURLToPath(import.meta.url)), "..");

const isolated = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "credentialless" };

export default defineConfig({
  root: stage,
  base: "./",
  server: { headers: isolated },
  preview: { headers: isolated },
  build: {
    rollupOptions: { input: join(stage, "bench", "index.html") },
  },
});
