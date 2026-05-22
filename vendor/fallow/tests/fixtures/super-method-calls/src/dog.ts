import { Animal } from './animal';

export class Dog extends Animal {
  bark(): string {
    return `${super.speak()} Woof!`;
  }
}
