import { Component, computed, input } from '@angular/core';

@Component({
  selector: 'app-inline',
  template: `
    @if (withAlias(); as aliased) {
      <p>{{ aliased }}</p>
    }
    @if (withoutAlias()) {
      <p>{{ withoutAlias() }}</p>
    }
  `,
})
export class InlineTemplateComponent {
  readonly name = input.required<string>();

  readonly withAlias = computed(() => {
    const n = this.name();
    return n ? `Hello, ${n}!` : undefined;
  });

  readonly withoutAlias = computed(() => {
    const n = this.name();
    return n ? `Goodbye, ${n}!` : undefined;
  });

  readonly genuinelyUnused = computed(() => 'never referenced');
}
