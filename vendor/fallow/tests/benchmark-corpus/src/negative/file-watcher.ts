// Negative: NOT a clone — file system watcher, structurally different from anything else
import { watch, FSWatcher, Stats } from 'fs';
import { readdir, stat } from 'fs/promises';
import { join, relative, extname } from 'path';
import { EventEmitter } from 'events';

interface WatchEvent {
  type: 'create' | 'modify' | 'delete' | 'rename';
  path: string;
  relativePath: string;
  timestamp: number;
  stats?: Stats;
}

interface WatchOptions {
  recursive: boolean;
  extensions?: string[];
  ignorePatterns?: RegExp[];
  debounceMs?: number;
  persistent?: boolean;
}

export class FileWatcher extends EventEmitter {
  private watchers: Map<string, FSWatcher> = new Map();
  private debounceTimers: Map<string, ReturnType<typeof setTimeout>> = new Map();
  private readonly rootDir: string;
  private readonly options: Required<WatchOptions>;
  private isWatching = false;

  constructor(rootDir: string, options?: Partial<WatchOptions>) {
    super();
    this.rootDir = rootDir;
    this.options = {
      recursive: true,
      extensions: [],
      ignorePatterns: [/node_modules/, /\.git/, /dist/],
      debounceMs: 100,
      persistent: true,
      ...options,
    };
  }

  async start(): Promise<void> {
    if (this.isWatching) return;

    this.isWatching = true;
    await this.watchDirectory(this.rootDir);
    this.emit('ready');
  }

  stop(): void {
    for (const [path, watcher] of this.watchers) {
      watcher.close();
      this.watchers.delete(path);
    }

    for (const timer of this.debounceTimers.values()) {
      clearTimeout(timer);
    }
    this.debounceTimers.clear();

    this.isWatching = false;
    this.emit('close');
  }

  private async watchDirectory(dir: string): Promise<void> {
    if (this.shouldIgnore(dir)) return;

    try {
      const watcher = watch(dir, { persistent: this.options.persistent }, (eventType, filename) => {
        if (filename) {
          this.handleEvent(eventType, join(dir, filename));
        }
      });

      this.watchers.set(dir, watcher);

      if (this.options.recursive) {
        const entries = await readdir(dir, { withFileTypes: true });
        for (const entry of entries) {
          if (entry.isDirectory()) {
            await this.watchDirectory(join(dir, entry.name));
          }
        }
      }
    } catch (error) {
      this.emit('error', { path: dir, error });
    }
  }

  private handleEvent(eventType: string, filePath: string): void {
    if (this.shouldIgnore(filePath)) return;
    if (!this.matchesExtension(filePath)) return;

    const existing = this.debounceTimers.get(filePath);
    if (existing) clearTimeout(existing);

    const timer = setTimeout(async () => {
      this.debounceTimers.delete(filePath);

      try {
        const stats = await stat(filePath).catch(() => null);
        const event: WatchEvent = {
          type: this.mapEventType(eventType, stats),
          path: filePath,
          relativePath: relative(this.rootDir, filePath),
          timestamp: Date.now(),
          stats: stats ?? undefined,
        };

        this.emit('change', event);
        this.emit(event.type, event);
      } catch (error) {
        this.emit('error', { path: filePath, error });
      }
    }, this.options.debounceMs);

    this.debounceTimers.set(filePath, timer);
  }

  private mapEventType(fsEvent: string, stats: Stats | null): WatchEvent['type'] {
    if (!stats) return 'delete';
    if (fsEvent === 'rename') return 'rename';
    return 'modify';
  }

  private shouldIgnore(path: string): boolean {
    return this.options.ignorePatterns.some((pattern) => pattern.test(path));
  }

  private matchesExtension(path: string): boolean {
    if (this.options.extensions.length === 0) return true;
    const ext = extname(path);
    return this.options.extensions.includes(ext);
  }

  getWatchedPaths(): string[] {
    return Array.from(this.watchers.keys());
  }

  get watching(): boolean {
    return this.isWatching;
  }
}
