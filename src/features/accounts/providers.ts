import type { AccountProviderId } from "./model/accountsStore";

export interface AccountProviderInfo {
  id: AccountProviderId;
  name: string;
  /** Why the app needs this account. */
  purpose: string;
}

/** Platforms whose accounts can be connected. A new one also needs its provider in `auth/account.rs`. */
export const ACCOUNT_PROVIDERS: AccountProviderInfo[] = [
  {
    id: "youtube",
    name: "YouTube",
    purpose: "Used for downloads from YouTube and YouTube Music.",
  },
];
