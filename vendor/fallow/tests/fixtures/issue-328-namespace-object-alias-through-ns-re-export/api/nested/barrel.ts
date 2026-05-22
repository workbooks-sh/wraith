// Namespace re-export with renaming. The consumer reaches `inner` through
// `API.foo.inner`; deeper accesses (`.used`) must credit `./leaf.ts`'s exports.
export * as inner from './leaf';

// Multi-hop chain entry. The consumer reaches `outer` through `API.foo.outer`,
// and `outer` is itself a namespace re-export barrel. Two-hop access
// `API.foo.outer.deep.deepUsed` must propagate through both re-exports.
export * as outer from './deeper-barrel';

// A sibling namespace re-export the consumer does NOT touch; its target
// `siblingLeaf` export must stay flagged as unused (negative control: the
// chain walker should not over-credit other namespace re-exports).
export * as untouched from './sibling';
