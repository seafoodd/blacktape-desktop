import { useEffect, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { FaYoutube } from "react-icons/fa";
import { MdCheckCircle, MdErrorOutline, MdRemoveCircleOutline } from "react-icons/md";
import {
  EMPTY_ACCOUNT,
  describeAccount,
  useAccountsStore,
  type AccountProviderId,
} from "../model/accountsStore";
import { ACCOUNT_PROVIDERS, type AccountProviderInfo } from "../providers";
import styles from "./accounts.module.css";

const ICONS: Record<AccountProviderId, ReactNode> = {
  youtube: <FaYoutube size={22} />,
};

interface Props {
  buttonClassName: string;
}

const AccountRow = ({ info, buttonClassName }: { info: AccountProviderInfo } & Props) => {
  const account = useAccountsStore((s) => s.accounts[info.id]) ?? EMPTY_ACCOUNT;
  const { check, connect, disconnect } = useAccountsStore();
  const busy = account.action !== null;
  const state = account.action ? null : account.status?.state;

  async function handleSignOut() {
    const confirmed = await ask(
      `Sign out of ${info.name} in Blacktape? Downloads from ${info.name} will ask you to sign in again.`,
      { title: "Sign out", kind: "warning" },
    );
    if (confirmed) await disconnect(info.id);
  }

  return (
    <div className={styles.row}>
      <div className={styles.icon}>{ICONS[info.id]}</div>
      <div className={styles.info}>
        <span className={styles.name}>{info.name}</span>
        <span className={styles.purpose}>{info.purpose}</span>
        <span
          className={`${styles.status} ${(state && styles[state]) || ""}`}
          role="status"
        >
          {state === "connected" && <MdCheckCircle />}
          {state === "disconnected" && <MdRemoveCircleOutline />}
          {state === "error" && <MdErrorOutline />}
          {describeAccount(account)}
          {account.checkedAt && !busy && (
            <span className={styles.time}>
              {" "}
              · {new Date(account.checkedAt).toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </span>
          )}
        </span>
      </div>
      <div className={styles.actions}>
        {/* Nothing to verify while signed out: Sign in is the only useful action. */}
        {account.status?.state !== "disconnected" && (
          <button className={buttonClassName} onClick={() => check(info.id)} disabled={busy}>
            Check
          </button>
        )}
        {state === "connected" ? (
          <button className={buttonClassName} onClick={handleSignOut} disabled={busy}>
            Sign out
          </button>
        ) : (
          <button className={buttonClassName} onClick={() => connect(info.id)} disabled={busy}>
            Sign in
          </button>
        )}
      </div>
    </div>
  );
};

/** Settings → Accounts: one row per platform, checked when the tab opens. */
export const AccountsSettings = ({ buttonClassName }: Props) => {
  const check = useAccountsStore((s) => s.check);

  useEffect(() => {
    ACCOUNT_PROVIDERS.forEach((p) => check(p.id));
    // A sign-in from a download (main window) or a sign-out changes the status: check it again.
    const unlisten = listen<AccountProviderId>("account-changed", (event) => {
      check(event.payload);
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [check]);

  return (
    <>
      {ACCOUNT_PROVIDERS.map((info) => (
        <AccountRow key={info.id} info={info} buttonClassName={buttonClassName} />
      ))}
    </>
  );
};
