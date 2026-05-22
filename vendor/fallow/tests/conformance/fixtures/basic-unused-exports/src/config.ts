export interface UsedConfig {
  debug: boolean;
}

export interface UnusedConfig {
  verbose: boolean;
  timeout: number;
}

export type UnusedAlias = string | number;
