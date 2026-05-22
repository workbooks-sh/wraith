export type ApiResponse = {
  data: unknown;
  status: number;
};

export type LegacyResponse = {
  result: unknown;
  code: number;
};

export type DeprecatedConfig = {
  apiKey: string;
  endpoint: string;
};
