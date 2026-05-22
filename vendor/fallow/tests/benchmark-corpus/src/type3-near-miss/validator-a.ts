// Type-3 clone: Near-miss of validator-b.ts (added/removed/modified statements)
interface ValidationRule {
  field: string;
  type: 'required' | 'minLength' | 'maxLength' | 'pattern' | 'custom';
  value?: unknown;
  message: string;
}

interface ValidationError {
  field: string;
  rule: string;
  message: string;
}

interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
}

export class FormValidator {
  private rules: Map<string, ValidationRule[]> = new Map();

  addRule(field: string, rule: ValidationRule): void {
    const existing = this.rules.get(field) ?? [];
    existing.push(rule);
    this.rules.set(field, existing);
  }

  removeRules(field: string): void {
    this.rules.delete(field);
  }

  validate(data: Record<string, unknown>): ValidationResult {
    const errors: ValidationError[] = [];

    for (const [field, rules] of this.rules) {
      const value = data[field];

      for (const rule of rules) {
        const error = this.checkRule(field, value, rule);
        if (error) {
          errors.push(error);
        }
      }
    }

    return { valid: errors.length === 0, errors };
  }

  private checkRule(field: string, value: unknown, rule: ValidationRule): ValidationError | null {
    switch (rule.type) {
      case 'required':
        if (value === undefined || value === null || value === '') {
          return { field, rule: 'required', message: rule.message };
        }
        break;

      case 'minLength':
        if (typeof value === 'string' && value.length < (rule.value as number)) {
          return { field, rule: 'minLength', message: rule.message };
        }
        break;

      case 'maxLength':
        if (typeof value === 'string' && value.length > (rule.value as number)) {
          return { field, rule: 'maxLength', message: rule.message };
        }
        break;

      case 'pattern':
        if (typeof value === 'string' && !(rule.value as RegExp).test(value)) {
          return { field, rule: 'pattern', message: rule.message };
        }
        break;

      case 'custom':
        if (typeof rule.value === 'function' && !rule.value(value)) {
          return { field, rule: 'custom', message: rule.message };
        }
        break;
    }

    return null;
  }

  validateField(field: string, value: unknown): ValidationError[] {
    const rules = this.rules.get(field);
    if (!rules) return [];

    const errors: ValidationError[] = [];
    for (const rule of rules) {
      const error = this.checkRule(field, value, rule);
      if (error) {
        errors.push(error);
      }
    }
    return errors;
  }

  hasRules(field: string): boolean {
    return this.rules.has(field) && (this.rules.get(field)?.length ?? 0) > 0;
  }

  getRuleCount(): number {
    let count = 0;
    for (const rules of this.rules.values()) {
      count += rules.length;
    }
    return count;
  }

  clear(): void {
    this.rules.clear();
  }
}
