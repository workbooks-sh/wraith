import { Animal } from './animal';

export class Cat extends Animal {
  meow(): string {
    return `${super.speak()} Meow!`;
  }
}
