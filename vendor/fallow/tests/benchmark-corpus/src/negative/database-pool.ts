// Negative: NOT a clone — database connection pool, unique domain logic
interface ConnectionConfig {
  host: string;
  port: number;
  database: string;
  username: string;
  password: string;
  ssl?: boolean;
}

interface PoolConfig {
  minConnections: number;
  maxConnections: number;
  acquireTimeout: number;
  idleTimeout: number;
  maxLifetime: number;
}

interface Connection {
  id: string;
  createdAt: number;
  lastUsedAt: number;
  isIdle: boolean;
  queries: number;
}

interface QueryResult<T = unknown> {
  rows: T[];
  rowCount: number;
  elapsed: number;
}

export class ConnectionPool {
  private idle: Connection[] = [];
  private active = new Map<string, Connection>();
  private waiting: Array<{ resolve: (conn: Connection) => void; reject: (err: Error) => void; timer: ReturnType<typeof setTimeout> }> = [];
  private readonly connectionConfig: ConnectionConfig;
  private readonly poolConfig: PoolConfig;
  private nextId = 0;
  private closed = false;
  private maintenanceTimer: ReturnType<typeof setInterval> | null = null;

  constructor(connectionConfig: ConnectionConfig, poolConfig?: Partial<PoolConfig>) {
    this.connectionConfig = connectionConfig;
    this.poolConfig = {
      minConnections: 2,
      maxConnections: 10,
      acquireTimeout: 5000,
      idleTimeout: 30000,
      maxLifetime: 3600000,
      ...poolConfig,
    };
  }

  async initialize(): Promise<void> {
    const promises: Promise<void>[] = [];
    for (let i = 0; i < this.poolConfig.minConnections; i++) {
      promises.push(this.createConnection());
    }
    await Promise.all(promises);

    this.maintenanceTimer = setInterval(() => this.runMaintenance(), 10000);
  }

  async acquire(): Promise<Connection> {
    if (this.closed) throw new Error('Pool is closed');

    // Try idle connection first
    while (this.idle.length > 0) {
      const conn = this.idle.pop()!;
      if (this.isConnectionValid(conn)) {
        conn.isIdle = false;
        conn.lastUsedAt = Date.now();
        this.active.set(conn.id, conn);
        return conn;
      }
    }

    // Create new if under limit
    const totalConnections = this.idle.length + this.active.size;
    if (totalConnections < this.poolConfig.maxConnections) {
      await this.createConnection();
      const conn = this.idle.pop()!;
      conn.isIdle = false;
      conn.lastUsedAt = Date.now();
      this.active.set(conn.id, conn);
      return conn;
    }

    // Wait for available connection
    return new Promise<Connection>((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = this.waiting.findIndex((w) => w.resolve === resolve);
        if (index !== -1) this.waiting.splice(index, 1);
        reject(new Error('Acquire timeout'));
      }, this.poolConfig.acquireTimeout);

      this.waiting.push({ resolve, reject, timer });
    });
  }

  release(conn: Connection): void {
    this.active.delete(conn.id);
    conn.isIdle = true;
    conn.lastUsedAt = Date.now();

    if (this.waiting.length > 0) {
      const waiter = this.waiting.shift()!;
      clearTimeout(waiter.timer);
      conn.isIdle = false;
      this.active.set(conn.id, conn);
      waiter.resolve(conn);
    } else {
      this.idle.push(conn);
    }
  }

  async query<T>(sql: string, params?: unknown[]): Promise<QueryResult<T>> {
    const conn = await this.acquire();
    const startTime = performance.now();

    try {
      // Simulate query execution
      conn.queries++;
      await new Promise((resolve) => setTimeout(resolve, 1));

      return {
        rows: [] as T[],
        rowCount: 0,
        elapsed: performance.now() - startTime,
      };
    } finally {
      this.release(conn);
    }
  }

  async close(): Promise<void> {
    this.closed = true;
    if (this.maintenanceTimer) {
      clearInterval(this.maintenanceTimer);
    }

    for (const waiter of this.waiting) {
      clearTimeout(waiter.timer);
      waiter.reject(new Error('Pool closing'));
    }
    this.waiting = [];
    this.idle = [];
    this.active.clear();
  }

  private async createConnection(): Promise<void> {
    const conn: Connection = {
      id: `conn_${this.nextId++}`,
      createdAt: Date.now(),
      lastUsedAt: Date.now(),
      isIdle: true,
      queries: 0,
    };
    this.idle.push(conn);
  }

  private isConnectionValid(conn: Connection): boolean {
    const age = Date.now() - conn.createdAt;
    if (age > this.poolConfig.maxLifetime) return false;
    return true;
  }

  private runMaintenance(): void {
    const now = Date.now();

    // Remove expired idle connections (keep minimum)
    while (this.idle.length > this.poolConfig.minConnections) {
      const oldest = this.idle[0];
      if (now - oldest.lastUsedAt > this.poolConfig.idleTimeout) {
        this.idle.shift();
      } else {
        break;
      }
    }
  }

  getStats(): { idle: number; active: number; waiting: number; total: number } {
    return {
      idle: this.idle.length,
      active: this.active.size,
      waiting: this.waiting.length,
      total: this.idle.length + this.active.size,
    };
  }
}
