import { API } from '@foo/bar';

// Single hop through a namespace re-export:
//   API.foo (alias suffix) -> barrel.ts
//   .inner (export * as inner from './leaf') -> leaf.ts
//   .used (the final export)
export const usedLeaf = API.foo.inner.used;

// Multi-hop chain (two consecutive namespace re-exports):
//   API.foo (alias suffix) -> barrel.ts
//   .outer (export * as outer from './deeper-barrel') -> deeper-barrel.ts
//   .deep (export * as deep from './deeper-leaf') -> deeper-leaf.ts
//   .deepUsed (the final export)
export const usedDeep = API.foo.outer.deep.deepUsed;
