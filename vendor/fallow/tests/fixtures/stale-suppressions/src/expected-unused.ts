// STALE @expected-unused: this export IS used by index.ts
/** @expected-unused */
export const usedExport = 'actually used';

// NOT STALE @expected-unused: this export IS unused (tag is working)
/** @expected-unused */
export const genuinelyUnused = 'nobody imports this';
