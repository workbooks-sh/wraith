import { Directive } from '@angular/core';
import { FormControl } from '@angular/forms';

@Directive()
export abstract class BaseFieldHandlerDirective {
  control = new FormControl('');

  // Used in child template via (blur)="trimValue()"
  trimValue(): void {
    const val = this.control.value;
    if (typeof val === 'string') {
      this.control.setValue(val.trim());
    }
  }

  // Used in child template via [class]="tooltipClass"
  readonly tooltipClass = 'tooltip-content';

  // NOT used anywhere, genuinely unused (control case)
  unusedBaseMethod(): void {
    // empty
  }
}
