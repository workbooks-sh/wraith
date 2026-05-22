// Type-3 clone: Near-miss of validator-a.ts (added logging, extra rule types, modified error format)
interface ValidationRule {
  field: string;
  type: 'required' | 'minLength' | 'maxLength' | 'pattern' | 'custom' | 'email' | 'range';
  value?: unknown;
  message: string;
}

interface ValidationError {
  field: string;
  rule: string;
  message: string;
  severity: 'error' | 'warning';
  timestamp: number;
}

interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
  warnings: ValidationError[];
  validatedAt: number;
}

export class FormValidator {
  private rules: Map<string, ValidationRule[]> = new Map();
  private validationCount = 0;

  addRule(field: string, rule: ValidationRule): void {
    const existing = this.rules.get(field) ?? [];
    existing.push(rule);
    this.rules.set(field, existing);
    console.debug(`Rule added for field "${field}": ${rule.type}`);
  }

  removeRules(field: string): void {
    this.rules.delete(field);
    console.debug(`Rules removed for field "${field}"`);
  }

  validate(data: Record<string, unknown>): ValidationResult {
    this.validationCount++;
    const errors: ValidationError[] = [];
    const warnings: ValidationError[] = [];

    for (const [field, rules] of this.rules) {
      const value = data[field];

      for (const rule of rules) {
        const error = this.checkRule(field, value, rule);
        if (error) {
          if (error.severity === 'warning') {
            warnings.push(error);
          } else {
            errors.push(error);
          }
        }
      }
    }

    return { valid: errors.length === 0, errors, warnings, validatedAt: Date.now() };
  }

  private checkRule(field: string, value: unknown, rule: ValidationRule): ValidationError | null {
    switch (rule.type) {
      case 'required':
        if (value === undefined || value === null || value === '') {
          return { field, rule: 'required', message: rule.message, severity: 'error', timestamp: Date.now() };
        }
        break;

      case 'minLength':
        if (typeof value === 'string' && value.length < (rule.value as number)) {
          return { field, rule: 'minLength', message: rule.message, severity: 'error', timestamp: Date.now() };
        }
        break;

      case 'maxLength':
        if (typeof value === 'string' && value.length > (rule.value as number)) {
          return { field, rule: 'maxLength', message: rule.message, severity: 'warning', timestamp: Date.now() };
        }
        break;

      case 'pattern':
        if (typeof value === 'string' && !(rule.value as RegExp).test(value)) {
          return { field, rule: 'pattern', message: rule.message, severity: 'error', timestamp: Date.now() };
        }
        break;

      case 'email': {
        const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
        if (typeof value === 'string' && !emailRegex.test(value)) {
          return { field, rule: 'email', message: rule.message, severity: 'error', timestamp: Date.now() };
        }
        break;
      }

      case 'range': {
        const [min, max] = rule.value as [number, number];
        if (typeof value === 'number' && (value < min || value > max)) {
          return { field, rule: 'range', message: rule.message, severity: 'error', timestamp: Date.now() };
        }
        break;
      }

      case 'custom':
        if (typeof rule.value === 'function' && !rule.value(value)) {
          return { field, rule: 'custom', message: rule.message, severity: 'error', timestamp: Date.now() };
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

  getValidationCount(): number {
    return this.validationCount;
  }

  clear(): void {
    this.rules.clear();
    this.validationCount = 0;
  }
}
