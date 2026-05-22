export const deepClone = <T>(obj: T): T => JSON.parse(JSON.stringify(obj));

export const shallowMerge = <T extends object>(a: T, b: Partial<T>): T => ({
  ...a,
  ...b,
});
