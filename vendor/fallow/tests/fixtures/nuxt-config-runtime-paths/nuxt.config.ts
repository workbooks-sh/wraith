export default defineNuxtConfig({
  plugins: [
    "~/runtime/plain-plugin",
    { src: "~/runtime/object-plugin", mode: "client" },
  ],
  components: {
    dirs: [{ path: "~/feature/ui" }],
  },
});
