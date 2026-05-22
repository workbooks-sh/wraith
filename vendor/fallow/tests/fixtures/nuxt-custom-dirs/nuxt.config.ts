export default defineNuxtConfig({
  alias: {
    '@shared': './app/shared'
  },
  imports: {
    dirs: ['~/custom/composables']
  },
  components: [
    { path: '@/feature-components' }
  ]
});
