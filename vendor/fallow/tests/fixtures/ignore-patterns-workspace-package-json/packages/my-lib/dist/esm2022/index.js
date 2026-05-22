import { chunk } from 'lodash';
export const splitItems = (items, size) => chunk(items, size);
