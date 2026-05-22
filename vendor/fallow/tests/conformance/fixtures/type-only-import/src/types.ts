export type UsedType = {
  id: number;
  name: string;
};

export type UnusedType = {
  value: string;
};

export interface UnusedInterface {
  count: number;
}

// This type is only used via 'import type' in consumer.ts
// but that file is not imported from index
export type OrphanedType = {
  data: unknown;
};
