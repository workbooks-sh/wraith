const path = require('path');

module.exports = {
  context: path.resolve(__dirname, 'src'),
  entry: {
    app: {
      import: './app.ts',
      filename: 'app.js',
    },
  },
  resolve: {
    alias: {
      '@components': path.resolve(__dirname, 'src/components'),
      '@utils': path.join(__dirname, 'src/utils'),
    },
  },
};
