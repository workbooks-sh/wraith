import { BaseComponent } from '@repro/ui-kit/internal/base';

export class Modal extends BaseComponent {
  title: string;
  constructor(title: string) {
    super('dialog');
    this.title = title;
  }
  open(): string {
    return `Opening modal: ${this.title}`;
  }
}
