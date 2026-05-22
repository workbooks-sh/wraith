import adapter from '@sveltejs/adapter-auto';

export default {
  kit: {
    adapter: adapter(),
    alias: {
      $utils: './src/lib/utils'
    }
  }
};
