import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type AccountProviderId = "youtube";
export type AccountState = "connected" | "disconnected" | "error";
export type AccountAction = "checking" | "connecting" | "disconnecting";

export interface AccountStatus {
  provider: AccountProviderId;
  state: AccountState;
  message: string | null;
}

export interface ProviderAccount {
  status: AccountStatus | null;
  /** What is running for this provider right now, if anything. */
  action: AccountAction | null;
  /** When the status was last confirmed (ms since epoch). */
  checkedAt: number | null;
}

interface AccountsState {
  accounts: Partial<Record<AccountProviderId, ProviderAccount>>;
  check: (provider: AccountProviderId) => Promise<void>;
  connect: (provider: AccountProviderId) => Promise<void>;
  disconnect: (provider: AccountProviderId) => Promise<void>;
}

const COMMANDS: Record<AccountAction, string> = {
  checking: "account_status",
  connecting: "account_connect",
  disconnecting: "account_disconnect",
};

export const EMPTY_ACCOUNT: ProviderAccount = { status: null, action: null, checkedAt: null };

export const useAccountsStore = create<AccountsState>((set, get) => {
  const run = async (provider: AccountProviderId, action: AccountAction) => {
    const current = get().accounts[provider] ?? EMPTY_ACCOUNT;
    if (current.action) return;
    set((s) => ({ accounts: { ...s.accounts, [provider]: { ...current, action } } }));

    let status: AccountStatus;
    try {
      status = await invoke<AccountStatus>(COMMANDS[action], { provider });
    } catch (e) {
      status = { provider, state: "error", message: String(e) };
    }
    set((s) => ({
      accounts: {
        ...s.accounts,
        [provider]: { status, action: null, checkedAt: Date.now() },
      },
    }));
  };

  return {
    accounts: {},
    check: (provider) => run(provider, "checking"),
    connect: (provider) => run(provider, "connecting"),
    disconnect: (provider) => run(provider, "disconnecting"),
  };
});

/** One line for the settings row: what is going on or what the last result was. */
export function describeAccount(account: ProviderAccount): string {
  switch (account.action) {
    case "checking":
      return "Checking…";
    case "connecting":
      return "Waiting for sign-in…";
    case "disconnecting":
      return "Signing out…";
  }
  const status = account.status;
  if (!status) return "Not checked yet";
  if (status.state === "connected") return status.message ?? "Connected";
  if (status.state === "disconnected") return status.message ?? "Not connected";
  return status.message ?? "Something went wrong";
}
