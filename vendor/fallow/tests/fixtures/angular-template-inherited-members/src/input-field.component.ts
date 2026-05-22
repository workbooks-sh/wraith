import { Component } from '@angular/core';
import { ReactiveFormsModule } from '@angular/forms';
import { BaseFieldHandlerDirective } from './base-field-handler.component';

@Component({
  selector: 'app-input-field',
  imports: [ReactiveFormsModule],
  templateUrl: './input-field.component.html',
})
export class InputFieldComponent extends BaseFieldHandlerDirective {}
