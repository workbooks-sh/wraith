// Type-2 clone: Same structure as event-bus-a.ts but with different names
type MessageCallback<T = unknown> = (data: T) => void | Promise<void>;

interface Registration {
  uid: string;
  channel: string;
  callback: MessageCallback;
  singleUse: boolean;
  weight: number;
}

export class MessageBroker {
  private registrations: Map<string, Registration[]> = new Map();
  private counter = 0;
  private log: Array<{ channel: string; data: unknown; time: number }> = [];
  private readonly maxLogSize: number;

  constructor(maxLogSize = 100) {
    this.maxLogSize = maxLogSize;
  }

  subscribe<T>(channel: string, callback: MessageCallback<T>, weight = 0): string {
    const uid = `reg_${this.counter++}`;
    const registration: Registration = {
      uid,
      channel,
      callback: callback as MessageCallback,
      singleUse: false,
      weight,
    };

    const current = this.registrations.get(channel) ?? [];
    current.push(registration);
    current.sort((a, b) => b.weight - a.weight);
    this.registrations.set(channel, current);

    return uid;
  }

  subscribeOnce<T>(channel: string, callback: MessageCallback<T>, weight = 0): string {
    const uid = `reg_${this.counter++}`;
    const registration: Registration = {
      uid,
      channel,
      callback: callback as MessageCallback,
      singleUse: true,
      weight,
    };

    const current = this.registrations.get(channel) ?? [];
    current.push(registration);
    current.sort((a, b) => b.weight - a.weight);
    this.registrations.set(channel, current);

    return uid;
  }

  unsubscribe(registrationUid: string): boolean {
    for (const [channel, regs] of this.registrations) {
      const idx = regs.findIndex((r) => r.uid === registrationUid);
      if (idx !== -1) {
        regs.splice(idx, 1);
        if (regs.length === 0) {
          this.registrations.delete(channel);
        }
        return true;
      }
    }
    return false;
  }

  async publish<T>(channel: string, data: T): Promise<void> {
    this.log.push({ channel, data, time: Date.now() });
    if (this.log.length > this.maxLogSize) {
      this.log.shift();
    }

    const regs = this.registrations.get(channel);
    if (!regs || regs.length === 0) return;

    const expired: string[] = [];

    for (const reg of regs) {
      try {
        await reg.callback(data);
      } catch (err) {
        console.error(`Error in callback ${reg.uid} for channel ${channel}:`, err);
      }

      if (reg.singleUse) {
        expired.push(reg.uid);
      }
    }

    for (const uid of expired) {
      this.unsubscribe(uid);
    }
  }

  removeAllCallbacks(channel?: string): void {
    if (channel) {
      this.registrations.delete(channel);
    } else {
      this.registrations.clear();
    }
  }

  callbackCount(channel: string): number {
    return this.registrations.get(channel)?.length ?? 0;
  }

  channelNames(): string[] {
    return Array.from(this.registrations.keys());
  }

  getLog(channel?: string): Array<{ channel: string; data: unknown; time: number }> {
    if (channel) {
      return this.log.filter((entry) => entry.channel === channel);
    }
    return [...this.log];
  }

  clearLog(): void {
    this.log = [];
  }
}
