import { BaseCommand } from './contracts';

export class DeployCommand extends BaseCommand {
  async execute(): Promise<void> {}

  enterDeploy(): void {}

  exitDeploy(): void {}

  deployHandler(): void {}

  cleanup(): void {}
}
