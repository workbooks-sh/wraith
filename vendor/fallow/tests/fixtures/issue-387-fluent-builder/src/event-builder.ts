type EventPayload = {
  eventVersion?: number;
  processId?: string;
  subject?: string;
};

export class EventBuilder {
  private payload: EventPayload = {};

  static create(): EventBuilder {
    return new EventBuilder();
  }

  static createWithDefaults(): EventBuilder {
    return EventBuilder.create()
      .setEventVersion(1)
      .setProcessId("default-id");
  }

  setEventVersion(value: number): EventBuilder {
    this.payload.eventVersion = value;
    return this;
  }

  setProcessId(value: string): EventBuilder {
    this.payload.processId = value;
    return this;
  }

  setSubject(value: string): EventBuilder {
    this.payload.subject = value;
    return this;
  }

  build(): EventPayload {
    return this.payload;
  }

  // Genuine unused fluent setter: must still be flagged after the fix.
  setUnused(value: string): EventBuilder {
    this.payload.subject = value;
    return this;
  }

  // Static helper that does NOT return an EventBuilder instance. Calls to
  // `EventBuilder.format(...).<anything>` must not piggy-back on the fluent
  // chain pass and credit `EventBuilder.<anything>`: `format` lacks the
  // `is_instance_returning_static` flag and the analyze-layer guard must
  // reject the chain.
  static format(value: string): string {
    return value.toUpperCase();
  }

  // Genuinely unused regular method on the class. The chain
  // `EventBuilder.format("x").trim()` MUST NOT credit this method, because
  // `.format()` does not return an EventBuilder instance.
  fakeFromNonFactory(): EventBuilder {
    return this;
  }
}
