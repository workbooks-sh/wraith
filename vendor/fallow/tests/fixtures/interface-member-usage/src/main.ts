import { FixedSizeScrollStrategy } from './fixed-size-strategy';
import { ScrollViewport } from './scroll-viewport';

const strategy = new FixedSizeScrollStrategy();
const viewport = new ScrollViewport(strategy);

viewport.initialize();
viewport.destroy();
