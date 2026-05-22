export const deepUsed = 'reached via API.foo.outer.deep.deepUsed';

// Negative control for the multi-hop case: the chain walker must stay
// per-member after two re-export hops.
export const deepUnused = 'no consumer reaches this through the chain';
