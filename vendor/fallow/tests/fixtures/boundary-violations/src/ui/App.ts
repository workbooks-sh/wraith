// Allowed: ui -> shared
import { helper } from '../shared/utils';

// Violation: ui -> db (not in allow list)
import { query } from '../db/query';

export const app = () => helper() + query();
