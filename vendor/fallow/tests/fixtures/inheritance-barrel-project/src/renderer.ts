import { RenderableShape } from './contracts/renderable-shape';

export class ShapeRenderer {
  private strategy: RenderableShape;

  constructor(strategy: RenderableShape) {
    this.strategy = strategy;
  }

  paint(): string {
    return this.strategy.render();
  }
}
