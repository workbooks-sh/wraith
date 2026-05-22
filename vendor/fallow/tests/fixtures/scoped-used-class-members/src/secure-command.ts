import { Authorizable, BaseCommand } from './contracts';

class SecureCommand extends BaseCommand implements Authorizable {
  async execute(): Promise<void> {}

  authorize(): boolean {
    return true;
  }

  hydrate(): void {}

  cleanup(): void {}
}

export { SecureCommand };
