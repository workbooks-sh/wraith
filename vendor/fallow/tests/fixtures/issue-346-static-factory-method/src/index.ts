import { MyClass, Service, Wrapper } from "./my-class";

const myInstance = MyClass.getInstance();
console.log(myInstance.getData());

const svc = Service.create();
svc.start();

// Cross-class factory: `Wrapper.getBuilder()` returns a `MyClass`, not a
// `Wrapper`. The accessed `getData` must be credited on `MyClass`, not on
// `Wrapper` (which has no `getData` and would silently be a no-op anyway,
// but the conservative behavior matters for cases where `Wrapper` does
// share the member name with the actually-returned class).
const wrapped = Wrapper.getBuilder();
console.log(wrapped.getData());
