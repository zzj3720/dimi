export interface ProviderChange {
  readonly providerId: string;
  readonly providerName: string;
  readonly added: number;
  readonly removed: number;
}

export interface RefreshResult {
  readonly changed: ProviderChange[];
  readonly unchanged: string[];
  readonly failed: Array<{ readonly provider: string; readonly reason: string }>;
}
