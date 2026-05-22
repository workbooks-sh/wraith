export default defineNuxtConfig({
  srcDir: 'src/',
  alias: {
    '@shared': './src/shared'
  },
  imports: {
    dirs: ['~/custom/composables']
  },
  components: [
    { path: '@/feature-components' }
  ]
});
