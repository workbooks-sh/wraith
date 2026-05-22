// Type-3 clone: Near-miss of queue-b.ts (some statements added/removed/reordered)
interface QueueItem<T> {
  data: T;
  priority: number;
  addedAt: number;
  id: string;
}

export class PriorityQueue<T> {
  private items: QueueItem<T>[] = [];
  private nextId = 0;

  enqueue(data: T, priority = 0): string {
    const id = `item_${this.nextId++}`;
    const item: QueueItem<T> = {
      data,
      priority,
      addedAt: Date.now(),
      id,
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
    return item?.data;
  }

  peek(): T | undefined {
    return this.items[0]?.data;
  }

  remove(id: string): boolean {
    const index = this.items.findIndex((item) => item.id === id);
    if (index === -1) return false;
    this.items.splice(index, 1);
    return true;
  }

  clear(): void {
    this.items = [];
    this.nextId = 0;
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
