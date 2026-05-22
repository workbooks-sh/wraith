export const usedHelper = (cfg: { debug: boolean }): string =>
  cfg.debug ? 'debug' : 'release';

export const unusedHelper = (): number => 42;

export function anotherUnusedHelper(): void {
  // This function is exported but never imported anywhere
}

export const yetAnotherUnused = 'nobody references this';
