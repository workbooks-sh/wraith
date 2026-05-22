import { EventBuilder } from "./event-builder";

const event = EventBuilder.createWithDefaults()
  .setProcessId("runtime-id")
  .setSubject("example.subject")
  .build();

console.log(event.processId);

// Chain leaves the `EventBuilder` type at `.build()` (returns EventPayload).
// `EventPayload.processId` is a property, but the call here illustrates that
// the chain credit must stop at `.build()`. `eventVersion` is read from the
// payload object, NOT credited as a class member usage.
const payload = EventBuilder.create().build();
console.log(payload.eventVersion);

// Chain whose root method is NOT `is_instance_returning_static`. The visitor
// emits a fluent-chain sentinel for `.fakeFromNonFactory()`, and the
// analyze-layer guard (`!has_factory { return false; }`) MUST reject it so
// `EventBuilder.fakeFromNonFactory` stays reported as unused. The chained
// method name intentionally matches a real EventBuilder member to prove the
// guard, not member-name absence, is doing the rejecting. Static analysis
// doesn't typecheck this call, so an unsound runtime expression is fine here.
const formatted = EventBuilder.format("x").fakeFromNonFactory();
console.log(formatted);
