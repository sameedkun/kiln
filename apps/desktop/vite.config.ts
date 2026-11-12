import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri's usual dev port (P4-03).
  server: { port: 1420, strictPort: true },
  clearScreen: false,
  build: {
    // WebView2 is an evergreen Chromium.
    target: "chrome120",
    // Fonts stay files next to the page: nothing is fetched from the network (ADR 0051 §5).
    assetsInlineLimit: 0,
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
