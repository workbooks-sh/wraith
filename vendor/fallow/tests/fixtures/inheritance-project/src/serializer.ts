import { BaseShape } from './base-shape';

export default class extends BaseShape {
  readonly kind = 'Serializer';

  getArea(): number {
    return 0;
  }

  getPerimeter(): number {
    return 0;
  }

  toJSON(): string {
    return this.describe();
  }
}
