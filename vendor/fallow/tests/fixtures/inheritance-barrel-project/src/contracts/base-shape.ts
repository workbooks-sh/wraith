export abstract class BaseShape {
  abstract readonly kind: string;
  abstract area(): number;

  describe(): string {
    return `${this.kind}: area=${this.area().toFixed(2)}`;
  }
}
