// This file is an orphan: not imported from any entry point.
// It uses OrphanedType via import type.
import type { OrphanedType } from './types';

export const process = (data: OrphanedType): string => JSON.stringify(data);
