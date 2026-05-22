// Type-2 clone: Same structure as event-bus-b.ts but with different names
type EventHandler<T = unknown> = (payload: T) => void | Promise<void>;

interface Subscription {
  id: string;
  event: string;
  handler: EventHandler;
  once: boolean;
  priority: number;
}

export class EventBus {
  private subscriptions: Map<string, Subscription[]> = new Map();
  private nextId = 0;
  private history: Array<{ event: string; payload: unknown; timestamp: number }> = [];
  private readonly maxHistory: number;

  constructor(maxHistory = 100) {
    this.maxHistory = maxHistory;
  }

  on<T>(event: string, handler: EventHandler<T>, priority = 0): string {
    const id = `sub_${this.nextId++}`;
    const subscription: Subscription = {
      id,
      event,
      handler: handler as EventHandler,
      once: false,
      priority,
    };

    const existing = this.subscriptions.get(event) ?? [];
    existing.push(subscription);
    existing.sort((a, b) => b.priority - a.priority);
    this.subscriptions.set(event, existing);

    return id;
  }

  once<T>(event: string, handler: EventHandler<T>, priority = 0): string {
    const id = `sub_${this.nextId++}`;
    const subscription: Subscription = {
      id,
      event,
      handler: handler as EventHandler,
      once: true,
      priority,
    };

    const existing = this.subscriptions.get(event) ?? [];
    existing.push(subscription);
    existing.sort((a, b) => b.priority - a.priority);
    this.subscriptions.set(event, existing);

    return id;
  }

  off(subscriptionId: string): boolean {
    for (const [event, subs] of this.subscriptions) {
      const index = subs.findIndex((s) => s.id === subscriptionId);
      if (index !== -1) {
        subs.splice(index, 1);
        if (subs.length === 0) {
          this.subscriptions.delete(event);
        }
        return true;
      }
    }
    return false;
  }

  async emit<T>(event: string, payload: T): Promise<void> {
    this.history.push({ event, payload, timestamp: Date.now() });
    if (this.history.length > this.maxHistory) {
      this.history.shift();
    }

    const subs = this.subscriptions.get(event);
    if (!subs || subs.length === 0) return;

    const toRemove: string[] = [];

    for (const sub of subs) {
      try {
        await sub.handler(payload);
      } catch (error) {
        console.error(`Error in handler ${sub.id} for event ${event}:`, error);
      }

      if (sub.once) {
        toRemove.push(sub.id);
      }
    }

    for (const id of toRemove) {
      this.off(id);
    }
  }

  removeAllListeners(event?: string): void {
    if (event) {
      this.subscriptions.delete(event);
    } else {
      this.subscriptions.clear();
    }
  }

  listenerCount(event: string): number {
    return this.subscriptions.get(event)?.length ?? 0;
  }

  eventNames(): string[] {
    return Array.from(this.subscriptions.keys());
  }

  getHistory(event?: string): Array<{ event: string; payload: unknown; timestamp: number }> {
    if (event) {
      return this.history.filter((h) => h.event === event);
    }
    return [...this.history];
  }

  clearHistory(): void {
    this.history = [];
  }
}
