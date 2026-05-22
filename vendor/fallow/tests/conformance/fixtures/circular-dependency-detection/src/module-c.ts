import { createA } from './module-a';

export const createC = (): string => `C(${createA()})`;
