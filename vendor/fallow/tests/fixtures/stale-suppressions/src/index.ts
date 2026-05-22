import { usedHelper, anotherUsedExport } from './utils';
import { usedExport } from './expected-unused';
import { something } from './file-level';

console.log(usedHelper(), usedExport, something, anotherUsedExport);
