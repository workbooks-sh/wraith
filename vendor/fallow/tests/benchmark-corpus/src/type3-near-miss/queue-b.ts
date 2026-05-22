// Type-3 clone: Near-miss of queue-a.ts (extra methods, logging, modified order)
interface QueueItem<T> {
  data: T;
  priority: number;
  addedAt: number;
  id: string;
  retryCount: number;
}

export class PriorityQueue<T> {
  private items: QueueItem<T>[] = [];
  private nextId = 0;
  private processedCount = 0;

  enqueue(data: T, priority = 0): string {
    const id = `item_${this.nextId++}`;
    const item: QueueItem<T> = {
      data,
      priority,
      addedAt: Date.now(),
      id,
      retryCount: 0,
    };

    // Binary search for insertion point
    let low = 0;
    let high = this.items.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (this.items[mid].priority > priority) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }

    this.items.splice(low, 0, item);
    return id;
  }

  dequeue(): T | undefined {
    const item = this.items.shift();
    if (item) {
      this.processedCount++;
    }
    return item?.data;
  }

  peek(): T | undefined {
    return this.items[0]?.data;
  }

  peekItem(): QueueItem<T> | undefined {
    return this.items[0];
  }

  remove(id: string): boolean {
    const index = this.items.findIndex((item) => item.id === id);
    if (index === -1) return false;
    this.items.splice(index, 1);
    return true;
  }

  requeue(id: string): boolean {
    const index = this.items.findIndex((item) => item.id === id);
    if (index === -1) return false;
    const item = this.items[index];
    item.retryCount++;
    // Move to end of same priority
    this.items.splice(index, 1);
    let low = 0;
    let high = this.items.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (this.items[mid].priority >= item.priority) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    this.items.splice(low, 0, item);
    return true;
  }

  clear(): void {
    this.items = [];
    this.nextId = 0;
    this.processedCount = 0;
  }

  get size(): number {
    return this.items.length;
  }

  isEmpty(): boolean {
    return this.items.length === 0;
  }

  toArray(): T[] {
    return this.items.map((item) => item.data);
  }

  drain(): T[] {
    const result = this.items.map((item) => item.data);
    this.items = [];
    return result;
  }

  getProcessedCount(): number {
    return this.processedCount;
  }

  updatePriority(id: string, newPriority: number): boolean {
    const index = this.items.findIndex((item) => item.id === id);
    if (index === -1) return false;

    const item = this.items[index];
    this.items.splice(index, 1);

    item.priority = newPriority;

    let low = 0;
    let high = this.items.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (this.items[mid].priority > newPriority) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }

    this.items.splice(low, 0, item);
    return true;
  }
}
