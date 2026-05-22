import { chunk } from 'lodash';

export const splitItems = <T>(items: T[], size: number): T[][] => chunk(items, size);
