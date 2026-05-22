import { defineConfig } from "cypress";

export default defineConfig({
  e2e: {
    specPattern: ["tests/integration/**/*.cy.ts"],
    supportFile: "tests/support/index.ts",
  },
  component: {
    specPattern: "src/**/*.cy.ts",
    supportFile: false,
  },
});
