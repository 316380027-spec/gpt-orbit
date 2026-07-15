export type QuotaWindowKind = 'fiveHour' | 'weekly' | 'other';

export interface QuotaWindow {
  kind: QuotaWindowKind;
  usedPercent: number;
  remainingPercent: number;
  windowDurationMins: number;
  resetsAt: number | null;
}

export type RateLimitSource = 'read' | 'updated' | 'cache';

export interface RateLimitState {
  fiveHour: QuotaWindow | null;
  weekly: QuotaWindow | null;
  other: QuotaWindow[];
  planType: string | null;
  reachedType: string | null;
  fetchedAt: number;
  source: RateLimitSource;
  stale: boolean;
}

export type QuotaConnectionStatus =
  | 'starting'
  | 'live'
  | 'stale'
  | 'offline'
  | 'loginRequired';
