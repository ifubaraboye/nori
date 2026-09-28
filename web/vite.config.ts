import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  // Relative asset URLs so web/dist loads under Electron's file:// protocol.
  base: "./",
  server: {
    port: 3001,
  },
  preview: {
    port: 3001,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
