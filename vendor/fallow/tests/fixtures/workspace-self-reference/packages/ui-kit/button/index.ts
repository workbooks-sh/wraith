import { BaseComponent } from '@repro/ui-kit/internal/base';

export class Button extends BaseComponent {
  label: string;
  constructor(label: string) {
    super('button');
    this.label = label;
  }
  override render(): string {
    return `<button>${this.label}</button>`;
  }
}
