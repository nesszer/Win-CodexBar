/// <reference types="vitest" />
import { fileURLToPath } from "node:url";
import { defineConfig, searchForWorkspaceRoot } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  // The mocked-IPC proof page is a separate entry so the desktop bundle never
  // contains the mock bridge or its fixtures.
  const proof = mode === "proof";
  // Vitest runs with mode "test" and imports the proof bridge in its tests.
  const allowLocaleFiles = proof || mode === "test";
  return {
    plugins: [react()],
    server: {
      host: "127.0.0.1",
      port: 1420,
      strictPort: true,
      fs: allowLocaleFiles
        ? {
            // The proof bridge serves the shipped English strings from rust/src/locale.
            allow: [
              searchForWorkspaceRoot(process.cwd()),
              fileURLToPath(new URL("../../rust/src/locale", import.meta.url)),
            ],
          }
        : undefined,
    },
    build: proof
      ? {
          outDir: "dist-proof",
          rollupOptions: { input: fileURLToPath(new URL("./proof.html", import.meta.url)) },
        }
      : undefined,
    clearScreen: false,
    test: {
      environment: "jsdom",
      globals: true,
      setupFiles: ["./src/test/setup.ts"],
      include: ["src/**/*.{test,spec}.{ts,tsx}"],
    },
  };
});
