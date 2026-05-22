export const used = 'reached via API.foo.inner.used';

// Negative control: same file, different export, never accessed. Must stay
// flagged so the credit path is precise (per-member, not whole-file).
export const stillUnused = 'no consumer reaches this';
