export default defineNuxtPlugin(() => ({
  provide: {
    greeting: () => "hello",
  },
}));

export const unusedPluginHelper = "still-unused";
