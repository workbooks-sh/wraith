import { BaseShape } from './base-shape';

export class Circle extends BaseShape {
  readonly kind = 'Circle';

  constructor(private radius: number) {
    super();
  }

  getArea(): number {
    return Math.PI * this.radius ** 2;
  }

  getPerimeter(): number {
    return 2 * Math.PI * this.radius;
  }
}
