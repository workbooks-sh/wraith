import { describe, expect, it } from "vitest";

import { covered } from "../src/covered";

describe("covered", () => {
  it("is test reachable", () => {
    expect(covered()).toBe("covered");
  });
});
