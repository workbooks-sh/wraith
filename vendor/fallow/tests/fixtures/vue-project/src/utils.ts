export const helper = () => 'help';
export const formatDate = (d: Date) => d.toISOString();
export const handlers = {
  click: () => 'clicked',
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
