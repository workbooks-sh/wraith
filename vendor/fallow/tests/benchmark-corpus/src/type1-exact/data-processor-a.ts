// Type-1 clone: Exact duplicate of data-processor-b.ts
import { EventEmitter } from 'events';

interface DataRecord {
  id: string;
  timestamp: number;
  payload: Record<string, unknown>;
  metadata: {
    source: string;
    version: number;
    tags: string[];
  };
}

interface ProcessingResult {
  processed: number;
  failed: number;
  skipped: number;
  duration: number;
}

export class DataProcessor extends EventEmitter {
  private buffer: DataRecord[] = [];
  private readonly maxBufferSize: number;
  private readonly flushInterval: number;
  private timer: ReturnType<typeof setInterval> | null = null;
  private isProcessing = false;

  constructor(maxBufferSize = 1000, flushInterval = 5000) {
    super();
    this.maxBufferSize = maxBufferSize;
    this.flushInterval = flushInterval;
  }

  async start(): Promise<void> {
    if (this.timer) {
      throw new Error('Processor already started');
    }
    this.timer = setInterval(() => this.flush(), this.flushInterval);
    this.emit('started');
  }

  async stop(): Promise<void> {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
    await this.flush();
    this.emit('stopped');
  }

  async ingest(record: DataRecord): Promise<void> {
    if (!this.validateRecord(record)) {
      this.emit('invalid', record);
      return;
    }

    this.buffer.push(record);

    if (this.buffer.length >= this.maxBufferSize) {
      await this.flush();
    }
  }

  async flush(): Promise<ProcessingResult> {
    if (this.isProcessing || this.buffer.length === 0) {
      return { processed: 0, failed: 0, skipped: 0, duration: 0 };
    }

    this.isProcessing = true;
    const batch = this.buffer.splice(0);
    const startTime = Date.now();

    let processed = 0;
    let failed = 0;
    let skipped = 0;

    for (const record of batch) {
      try {
        if (this.shouldSkip(record)) {
          skipped++;
          continue;
        }

        const transformed = this.transform(record);
        await this.persist(transformed);
        processed++;
      } catch (error) {
        failed++;
        this.emit('error', { record, error });
      }
    }

    const duration = Date.now() - startTime;
    this.isProcessing = false;

    const result: ProcessingResult = { processed, failed, skipped, duration };
    this.emit('flushed', result);
    return result;
  }

  private validateRecord(record: DataRecord): boolean {
    if (!record.id || typeof record.id !== 'string') return false;
    if (!record.timestamp || record.timestamp <= 0) return false;
    if (!record.payload || typeof record.payload !== 'object') return false;
    if (!record.metadata?.source) return false;
    return true;
  }

  private shouldSkip(record: DataRecord): boolean {
    const age = Date.now() - record.timestamp;
    const maxAge = 24 * 60 * 60 * 1000; // 24 hours
    if (age > maxAge) return true;
    if (record.metadata.tags.includes('test')) return true;
    return false;
  }

  private transform(record: DataRecord): DataRecord {
    return {
      ...record,
      payload: {
        ...record.payload,
        processedAt: Date.now(),
        processorVersion: '2.0',
      },
      metadata: {
        ...record.metadata,
        version: record.metadata.version + 1,
      },
    };
  }

  private async persist(record: DataRecord): Promise<void> {
    // Simulate async persistence
    await new Promise((resolve) => setTimeout(resolve, 1));
    this.emit('persisted', record.id);
  }

  getBufferSize(): number {
    return this.buffer.length;
  }

  isRunning(): boolean {
    return this.timer !== null;
  }
}
