import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  EMPTY_ACCOUNT,
  describeAccount,
  useAccountsStore,
  type AccountStatus,
  type ProviderAccount,
} from "./accountsStore.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const deferred = <T>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

const connectedStatus: AccountStatus = { provider: "youtube", state: "connected", message: null };

beforeEach(() => {
  invokeMock.mockReset();
  useAccountsStore.setState({ accounts: {} });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("account commands", () => {
  it("check calls account_status for the provider", async () => {
    invokeMock.mockResolvedValue(connectedStatus);
    await useAccountsStore.getState().check("youtube");
    expect(invokeMock).toHaveBeenCalledWith("account_status", { provider: "youtube" });
  });

  it("connect calls account_connect for the provider", async () => {
    invokeMock.mockResolvedValue(connectedStatus);
    await useAccountsStore.getState().connect("youtube");
    expect(invokeMock).toHaveBeenCalledWith("account_connect", { provider: "youtube" });
  });

  it("disconnect calls account_disconnect for the provider", async () => {
    invokeMock.mockResolvedValue(connectedStatus);
    await useAccountsStore.getState().disconnect("youtube");
    expect(invokeMock).toHaveBeenCalledWith("account_disconnect", { provider: "youtube" });
  });
});

describe("in-flight action", () => {
  it.each([
    ["check", "checking"],
    ["connect", "connecting"],
    ["disconnect", "disconnecting"],
  ] as const)("%s sets action to %s while pending", async (method, action) => {
    const d = deferred<AccountStatus>();
    invokeMock.mockReturnValue(d.promise as any);

    const pending = useAccountsStore.getState()[method]("youtube");

    expect(useAccountsStore.getState().accounts.youtube?.action).toBe(action);

    d.resolve(connectedStatus);
    await pending;
  });

  it("stores the resolved status, clears action and sets checkedAt", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(1_700_000_000_000));

    const status: AccountStatus = { provider: "youtube", state: "connected", message: null };
    invokeMock.mockResolvedValue(status);

    await useAccountsStore.getState().check("youtube");

    const account = useAccountsStore.getState().accounts.youtube;
    expect(account?.status).toEqual(status);
    expect(account?.action).toBeNull();
    expect(account?.checkedAt).toBe(1_700_000_000_000);
  });

  it("turns a rejection into an error status with the message", async () => {
    invokeMock.mockRejectedValue("boom");

    await useAccountsStore.getState().check("youtube");

    const account = useAccountsStore.getState().accounts.youtube;
    expect(account?.status).toEqual({ provider: "youtube", state: "error", message: "boom" });
    expect(account?.action).toBeNull();
    expect(typeof account?.checkedAt).toBe("number");
  });

  it("ignores a second check while one is pending", async () => {
    const d = deferred<AccountStatus>();
    invokeMock.mockReturnValue(d.promise as any);

    const store = useAccountsStore.getState();
    const first = store.check("youtube");
    const second = store.check("youtube");

    d.resolve(connectedStatus);
    await Promise.all([first, second]);

    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("ignores connect while a check is pending", async () => {
    const d = deferred<AccountStatus>();
    invokeMock.mockReturnValue(d.promise as any);

    const store = useAccountsStore.getState();
    const first = store.check("youtube");
    const second = store.connect("youtube");

    d.resolve(connectedStatus);
    await Promise.all([first, second]);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("account_status", { provider: "youtube" });
  });

  it("keeps the previous status while a new check is pending", async () => {
    const previous: AccountStatus = { provider: "youtube", state: "disconnected", message: "Signed out." };
    useAccountsStore.setState({
      accounts: { youtube: { ...EMPTY_ACCOUNT, status: previous } },
    });

    const d = deferred<AccountStatus>();
    invokeMock.mockReturnValue(d.promise as any);

    const pending = useAccountsStore.getState().check("youtube");

    const account = useAccountsStore.getState().accounts.youtube;
    expect(account?.status).toEqual(previous);
    expect(account?.action).toBe("checking");

    d.resolve(connectedStatus);
    await pending;
  });
});

describe("describeAccount", () => {
  const account = (overrides: Partial<ProviderAccount>): ProviderAccount => ({
    ...EMPTY_ACCOUNT,
    ...overrides,
  });

  it("describes checking", () => {
    expect(describeAccount(account({ action: "checking" }))).toBe("Checking…");
  });

  it("describes connecting", () => {
    expect(describeAccount(account({ action: "connecting" }))).toBe("Waiting for sign-in…");
  });

  it("describes disconnecting", () => {
    expect(describeAccount(account({ action: "disconnecting" }))).toBe("Signing out…");
  });

  it("prefers the action over an existing status", () => {
    const status: AccountStatus = { provider: "youtube", state: "connected", message: "x" };
    expect(describeAccount(account({ action: "checking", status }))).toBe("Checking…");
  });

  it("says not checked yet when there is no status", () => {
    expect(describeAccount(account({}))).toBe("Not checked yet");
  });

  it("says Connected for a connected status without a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "connected", message: null };
    expect(describeAccount(account({ status }))).toBe("Connected");
  });

  it("shows the message for a connected status with a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "connected", message: "x" };
    expect(describeAccount(account({ status }))).toBe("x");
  });

  it("says Not connected for a disconnected status without a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "disconnected", message: null };
    expect(describeAccount(account({ status }))).toBe("Not connected");
  });

  it("shows the message for a disconnected status with a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "disconnected", message: "Signed out." };
    expect(describeAccount(account({ status }))).toBe("Signed out.");
  });

  it("says Something went wrong for an error status without a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "error", message: null };
    expect(describeAccount(account({ status }))).toBe("Something went wrong");
  });

  it("shows the message for an error status with a message", () => {
    const status: AccountStatus = { provider: "youtube", state: "error", message: "No internet" };
    expect(describeAccount(account({ status }))).toBe("No internet");
  });
});
