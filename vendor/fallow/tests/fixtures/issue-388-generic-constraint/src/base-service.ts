import { BaseClient } from "./base-client";

export abstract class BaseService<TClient extends BaseClient> {
  constructor(protected readonly client: TClient) {}

  async getLatest(id: string): Promise<string> {
    return await this.client.fetchLatest(id);
  }
}
