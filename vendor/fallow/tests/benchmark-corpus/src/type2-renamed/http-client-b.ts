// Type-2 clone: Same structure as http-client-a.ts but with different variable/function names
interface FetchOptions {
  endpoint: string;
  verb: 'GET' | 'POST' | 'PUT' | 'DELETE';
  customHeaders?: Record<string, string>;
  payload?: unknown;
  timeoutMs?: number;
  retryCount?: number;
}

interface FetchResult<T> {
  body: T;
  statusCode: number;
  responseHeaders: Record<string, string>;
  latency: number;
}

export class ApiFetcher {
  private apiRoot: string;
  private baseHeaders: Record<string, string>;
  private timeoutDuration: number;
  private retryLimit: number;

  constructor(apiRoot: string, opts: { customHeaders?: Record<string, string>; timeoutMs?: number; retryCount?: number } = {}) {
    this.apiRoot = apiRoot;
    this.baseHeaders = opts.customHeaders ?? { 'Content-Type': 'application/json' };
    this.timeoutDuration = opts.timeoutMs ?? 30000;
    this.retryLimit = opts.retryCount ?? 3;
  }

  async fetchOne<T>(route: string, extraHeaders?: Record<string, string>): Promise<FetchResult<T>> {
    return this.performFetch<T>({
      endpoint: `${this.apiRoot}${route}`,
      verb: 'GET',
      customHeaders: { ...this.baseHeaders, ...extraHeaders },
      timeoutMs: this.timeoutDuration,
      retryCount: this.retryLimit,
    });
  }

  async create<T>(route: string, payload: unknown, extraHeaders?: Record<string, string>): Promise<FetchResult<T>> {
    return this.performFetch<T>({
      endpoint: `${this.apiRoot}${route}`,
      verb: 'POST',
      payload,
      customHeaders: { ...this.baseHeaders, ...extraHeaders },
      timeoutMs: this.timeoutDuration,
      retryCount: this.retryLimit,
    });
  }

  async update<T>(route: string, payload: unknown, extraHeaders?: Record<string, string>): Promise<FetchResult<T>> {
    return this.performFetch<T>({
      endpoint: `${this.apiRoot}${route}`,
      verb: 'PUT',
      payload,
      customHeaders: { ...this.baseHeaders, ...extraHeaders },
      timeoutMs: this.timeoutDuration,
      retryCount: this.retryLimit,
    });
  }

  async remove<T>(route: string, extraHeaders?: Record<string, string>): Promise<FetchResult<T>> {
    return this.performFetch<T>({
      endpoint: `${this.apiRoot}${route}`,
      verb: 'DELETE',
      customHeaders: { ...this.baseHeaders, ...extraHeaders },
      timeoutMs: this.timeoutDuration,
      retryCount: this.retryLimit,
    });
  }

  private async performFetch<T>(opts: FetchOptions): Promise<FetchResult<T>> {
    let recentError: Error | null = null;
    const maxAttempts = opts.retryCount ?? this.retryLimit;

    for (let tryNum = 0; tryNum <= maxAttempts; tryNum++) {
      try {
        const t0 = performance.now();
        const res = await this.rawFetch(opts);
        const latency = performance.now() - t0;

        if (res.statusCode >= 500 && tryNum < maxAttempts) {
          await this.wait(this.exponentialBackoff(tryNum));
          continue;
        }

        return {
          body: res.body as T,
          statusCode: res.statusCode,
          responseHeaders: res.responseHeaders,
          latency,
        };
      } catch (err) {
        recentError = err as Error;
        if (tryNum < maxAttempts) {
          await this.wait(this.exponentialBackoff(tryNum));
        }
      }
    }

    throw recentError ?? new Error('Fetch failed after retries');
  }

  private async rawFetch(opts: FetchOptions): Promise<{ body: unknown; statusCode: number; responseHeaders: Record<string, string> }> {
    const ac = new AbortController();
    const tid = setTimeout(() => ac.abort(), opts.timeoutMs ?? this.timeoutDuration);

    try {
      const res = await fetch(opts.endpoint, {
        method: opts.verb,
        headers: opts.customHeaders,
        body: opts.payload ? JSON.stringify(opts.payload) : undefined,
        signal: ac.signal,
      });

      const body = await res.json();
      const responseHeaders: Record<string, string> = {};
      res.headers.forEach((val, hdr) => {
        responseHeaders[hdr] = val;
      });

      return { body, statusCode: res.status, responseHeaders };
    } finally {
      clearTimeout(tid);
    }
  }

  private exponentialBackoff(tryNum: number): number {
    const baseDelay = 1000;
    const ceiling = 30000;
    const computed = Math.min(baseDelay * Math.pow(2, tryNum), ceiling);
    return computed + Math.random() * computed * 0.1;
  }

  private wait(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
