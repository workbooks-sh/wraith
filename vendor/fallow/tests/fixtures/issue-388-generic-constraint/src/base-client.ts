export abstract class BaseClient {
  async fetchLatest(id: string): Promise<string> {
    return `ok:${id}`;
  }

  async unusedBaseMethod(): Promise<void> {}
}
