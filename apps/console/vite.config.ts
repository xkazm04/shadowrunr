import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Port 1420 is the Tauri convention (tauri.conf.json devUrl). The daemon allows CORS from
// http://localhost:*, so `pnpm dev` works in a plain browser too.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", sourcemap: false, manifest: true },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/kit/testSetup.ts"],
  },
});
