import { Circle } from './circle';
import { ShapeRenderer } from './renderer';

const circle = new Circle(3);

console.log(circle.describe());
console.log(new ShapeRenderer(circle).paint());
