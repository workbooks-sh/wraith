// Type-2 clone: Same structure as http-client-b.ts but with different variable/function names
interface RequestConfig {
  url: string;
  method: 'GET' | 'POST' | 'PUT' | 'DELETE';
  headers?: Record<string, string>;
  body?: unknown;
  timeout?: number;
  retries?: number;
}

interface ApiResponse<T> {
  data: T;
  status: number;
  headers: Record<string, string>;
  elapsed: number;
}

export class HttpClient {
  private baseUrl: string;
  private defaultHeaders: Record<string, string>;
  private requestTimeout: number;
  private maxRetries: number;

  constructor(baseUrl: string, options: { headers?: Record<string, string>; timeout?: number; retries?: number } = {}) {
    this.baseUrl = baseUrl;
    this.defaultHeaders = options.headers ?? { 'Content-Type': 'application/json' };
    this.requestTimeout = options.timeout ?? 30000;
    this.maxRetries = options.retries ?? 3;
  }

  async get<T>(path: string, headers?: Record<string, string>): Promise<ApiResponse<T>> {
    return this.executeRequest<T>({
      url: `${this.baseUrl}${path}`,
      method: 'GET',
      headers: { ...this.defaultHeaders, ...headers },
      timeout: this.requestTimeout,
      retries: this.maxRetries,
    });
  }

  async post<T>(path: string, body: unknown, headers?: Record<string, string>): Promise<ApiResponse<T>> {
    return this.executeRequest<T>({
      url: `${this.baseUrl}${path}`,
      method: 'POST',
      body,
      headers: { ...this.defaultHeaders, ...headers },
      timeout: this.requestTimeout,
      retries: this.maxRetries,
    });
  }

  async put<T>(path: string, body: unknown, headers?: Record<string, string>): Promise<ApiResponse<T>> {
    return this.executeRequest<T>({
      url: `${this.baseUrl}${path}`,
      method: 'PUT',
      body,
      headers: { ...this.defaultHeaders, ...headers },
      timeout: this.requestTimeout,
      retries: this.maxRetries,
    });
  }

  async delete<T>(path: string, headers?: Record<string, string>): Promise<ApiResponse<T>> {
    return this.executeRequest<T>({
      url: `${this.baseUrl}${path}`,
      method: 'DELETE',
      headers: { ...this.defaultHeaders, ...headers },
      timeout: this.requestTimeout,
      retries: this.maxRetries,
    });
  }

  private async executeRequest<T>(config: RequestConfig): Promise<ApiResponse<T>> {
    let lastError: Error | null = null;
    const attempts = config.retries ?? this.maxRetries;

    for (let attempt = 0; attempt <= attempts; attempt++) {
      try {
        const startTime = performance.now();
        const response = await this.sendRequest(config);
        const elapsed = performance.now() - startTime;

        if (response.status >= 500 && attempt < attempts) {
          await this.delay(this.calculateBackoff(attempt));
          continue;
        }

        return {
          data: response.data as T,
          status: response.status,
          headers: response.headers,
          elapsed,
        };
      } catch (error) {
        lastError = error as Error;
        if (attempt < attempts) {
          await this.delay(this.calculateBackoff(attempt));
        }
      }
    }

    throw lastError ?? new Error('Request failed after retries');
  }

  private async sendRequest(config: RequestConfig): Promise<{ data: unknown; status: number; headers: Record<string, string> }> {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), config.timeout ?? this.requestTimeout);

    try {
      const response = await fetch(config.url, {
        method: config.method,
        headers: config.headers,
        body: config.body ? JSON.stringify(config.body) : undefined,
        signal: controller.signal,
      });

      const data = await response.json();
      const headers: Record<string, string> = {};
      response.headers.forEach((value, key) => {
        headers[key] = value;
      });

      return { data, status: response.status, headers };
    } finally {
      clearTimeout(timeoutId);
    }
  }

  private calculateBackoff(attempt: number): number {
    const base = 1000;
    const max = 30000;
    const delay = Math.min(base * Math.pow(2, attempt), max);
    return delay + Math.random() * delay * 0.1;
  }

  private delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
