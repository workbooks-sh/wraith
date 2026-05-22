// Negative control: the consumer never accesses `API.foo.untouched.*`, so
// every export of this file must stay flagged. Confirms the chain walker
// keys on the specific namespace re-export name the consumer touched.
export const siblingLeaf = 'should remain flagged';
