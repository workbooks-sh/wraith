import { BaseShape } from './base-shape';

export class Rectangle extends BaseShape {
  readonly kind = 'Rectangle';

  constructor(
    private width: number,
    private height: number,
  ) {
    super();
  }

  getArea(): number {
    return this.width * this.height;
  }

  getPerimeter(): number {
    return 2 * (this.width + this.height);
  }
}
