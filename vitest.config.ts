import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    css: { include: [/styles\.css/] },
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
