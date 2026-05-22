export class Animal {
  private name: string;

  constructor(name: string) {
    this.name = name;
  }

  speak(): string {
    return `${this.name} says`;
  }

  greet(): string {
    return `Hello, I'm ${this.name}`;
  }

  unusedOnParent(): string {
    return 'nobody calls this';
  }
}
