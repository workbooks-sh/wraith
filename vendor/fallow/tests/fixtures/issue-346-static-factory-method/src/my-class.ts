export class MyClass {
  static getInstance() {
    return new this();
  }

  getData() {
    return [1, 2, 3];
  }

  // Genuinely unused: must still be flagged after the fix. Guards against
  // over-crediting (e.g., crediting every member of the class when ANY
  // factory access is observed).
  unusedHelper() {
    return null;
  }
}

export class Service {
  static create() {
    return new Service();
  }

  start() {
    return "ok";
  }
}

// Factory that returns a different class. `getBuilder` is NOT
// `is_instance_returning_static` (it returns `new MyClass()`, not
// `new Wrapper()` or `new this()`), so the analyze layer must drop the
// sentinel binding silently and treat `Wrapper.getBuilder` like an ordinary
// static call (member access on its result must not credit `Wrapper`).
export class Wrapper {
  static getBuilder() {
    return new MyClass();
  }
}
