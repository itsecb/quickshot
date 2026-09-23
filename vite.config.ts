import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

const root = import.meta.dirname;

const host = process.env.TAURI_DEV_HOST;

// Multi-page app: every window the Rust side opens maps to one HTML entry here.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  resolve: {
    alias: { $lib: resolve(root, "src/lib"), $ui: resolve(root, "src/ui") },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    outDir: "dist",
    target: ["es2022", "chrome110", "safari16"],
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      input: {
        main: resolve(root, "index.html"),
        overlay: resolve(root, "overlay.html"),
        editor: resolve(root, "editor.html"),
        pin: resolve(root, "pin.html"),
        guide: resolve(root, "guide.html"),
        history: resolve(root, "history.html"),
        countdown: resolve(root, "countdown.html"),
        compare: resolve(root, "compare.html"),
        thumb: resolve(root, "thumb.html"),
      },
    },
  },
});
