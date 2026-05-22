import { expectType } from "tsd";

import type { Value } from "../../src/index";

expectType<Value>("ok");
