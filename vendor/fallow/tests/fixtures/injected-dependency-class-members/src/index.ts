import { FooClass } from "./foo";

class MyClass {
  constructor(private deps: { foo: FooClass }) {}

  test() {
    this.deps.foo.foo();
  }

  test2() {
    const foo = new FooClass();
    foo.bar();
  }
}

const myClass = new MyClass({
  foo: new FooClass(),
});

myClass.test();
