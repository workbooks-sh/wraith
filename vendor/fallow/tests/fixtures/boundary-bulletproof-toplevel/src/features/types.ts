// Top-level non-barrel file. Deliberately imports from `src/app/` to exercise
// strict mode: under the Bulletproof preset, top-level files inside
// `src/features/` classify as `features`, so this import must produce a
// boundary violation.
import { page } from '../app/page';
export const Toplevel = page;
