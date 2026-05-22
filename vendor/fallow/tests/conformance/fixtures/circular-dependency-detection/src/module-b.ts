import { createC } from './module-c';

export const createB = (): string => `B(${createC()})`;
