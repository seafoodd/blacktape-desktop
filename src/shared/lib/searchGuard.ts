/**
 * Stale-response guard for search requests.
 *
 * Every search takes a token from `begin()`. When its response arrives, it may only
 * update visible state if `isLatest(token)` still holds, i.e. no newer search has
 * started in the meantime. This prevents a slow response for an older query from
 * overwriting the results of the query the user is looking at now.
 */
export interface LatestGuard {
  /** Starts a new request and returns its token. Invalidates all earlier tokens. */
  begin(): number;
  /** True only for the token returned by the most recent `begin()` call. */
  isLatest(token: number): boolean;
}

export function createLatestGuard(): LatestGuard {
  let current = 0;
  return {
    begin() {
      current += 1;
      return current;
    },
    isLatest(token) {
      return current > 0 && token === current;
    },
  };
}
