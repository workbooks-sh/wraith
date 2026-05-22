import { BaseShape, RenderableShape } from './contracts';

export class Circle extends BaseShape implements RenderableShape {
  readonly kind = 'Circle';

  constructor(private readonly radius: number) {
    super();
  }

  area(): number {
    return Math.PI * this.radius ** 2;
  }

  render(): string {
    return this.describe();
  }

  unusedHelper(): string {
    return 'unused';
  }
}
