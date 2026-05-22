export const usedFn = (): string => 'used';

// fallow-ignore-next-line unused-export
export const suppressedUnused = (): string => 'suppressed';

// This one is NOT suppressed and should be reported
export const unsuppressedUnused = (): number => 42;
