import { Component, computed, input } from '@angular/core';

@Component({
  selector: 'app-external',
  templateUrl: './external.component.html',
})
export class ExternalTemplateComponent {
  readonly name = input.required<string>();

  readonly externalWithAlias = computed(() => {
    const n = this.name();
    return n ? `Hi, ${n}!` : undefined;
  });

  readonly externalUnused = computed(() => 'never referenced');
}
