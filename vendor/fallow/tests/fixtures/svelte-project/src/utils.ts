export const greet = () => 'hello';
export const formatName = (n: string) => n.toUpperCase();
export const isActive = true;
export const tooltip = () => ({});
export const inTernary = () => 'ternary';
export const inCallback = (n: number) => String(n);
export const inSpread = () => 'spread';
export const myAttach = (node: HTMLElement) => {
  node.dataset.attached = 'true';
};
export class Counter {
  value = 0;

  bump() {
    this.value += 1;
  }

  unused() {
    return this.value;
  }
}
export const unusedImported = () => 'still unused';
export const unusedUtil = () => 'unused';
