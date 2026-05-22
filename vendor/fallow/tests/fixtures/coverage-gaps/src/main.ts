import { covered } from "./covered";
import { viaFixture } from "./fixture-only";
import { viaSetup } from "./setup-only";

export const app = [covered(), viaSetup(), viaFixture()].join(":");
