export class BaseComponent {
  protected tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  render(): string {
    return `<${this.tag}></${this.tag}>`;
  }
}
