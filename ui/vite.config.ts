import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1430, strictPort: true },
  build: { target: "safari15", outDir: "dist", emptyOutDir: true },
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
