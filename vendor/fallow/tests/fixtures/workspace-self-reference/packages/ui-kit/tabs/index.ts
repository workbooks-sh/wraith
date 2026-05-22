import { BaseComponent } from '@repro/ui-kit/internal/base';

export class Tabs extends BaseComponent {
  items: string[];
  constructor(items: string[]) {
    super('tabs');
    this.items = items;
  }
  override render(): string {
    return this.items.map((i) => `<tab>${i}</tab>`).join('');
  }
}
