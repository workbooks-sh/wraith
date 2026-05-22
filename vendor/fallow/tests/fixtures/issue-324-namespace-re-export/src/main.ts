// Variant A: bug-report shape. Direct named import of a namespace re-export,
// member accessed via `MyNamespace.someExportedSymbol`.
import { MyNamespace } from './barrel';

// Variant B: multi-hop. The consumer imports through an outer named-re-export
// barrel; the actual `export * as Deep from './source-deep'` lives one level
// deeper. The forward-walk of re-export edges must connect them.
import { Deep } from './outer-barrel';

// Variant C: whole-object use. `Object.keys` consumes the namespace as an
// object literal; every export on the source must be credited.
import { Whole } from './whole-barrel';

console.log(MyNamespace.someExportedSymbol(1));
console.log(MyNamespace.anotherSymbol('hi'));

console.log(Deep.deepUsed(1));

console.log(Object.keys(Whole));
