import { createB } from './module-b';

export const createA = (): string => `A(${createB()})`;
