export abstract class BaseShape {
  abstract readonly kind: string;
  abstract getArea(): number;
  abstract getPerimeter(): number;

  describe(): string {
    return `${this.kind}: area=${this.getArea().toFixed(2)}, perimeter=${this.getPerimeter().toFixed(2)}`;
  }
}
