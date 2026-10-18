import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The stage is served as an OBS browser source later; `base: "./"` keeps the built bundle
// relocatable (file:// or any sub-path).
export default defineConfig({
  base: "./",
  plugins: [react()],
  server: { port: 5178, strictPort: false },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
