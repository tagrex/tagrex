import { mergeConfig } from 'vite';
import appConfig from '../../vite.config.js';

// Exercise the same compiler settings and chunking as the shipping frontend.
export default mergeConfig(appConfig, {
  root: '.',
  build: {
    outDir: '.test-dist',
    rolldownOptions: { input: ['tests/ui/fixture/index.html', 'tests/ui/dock-fixture/index.html', 'app/ui/index.html'] },
  },
});
