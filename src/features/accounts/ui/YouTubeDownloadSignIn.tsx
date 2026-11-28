import { useEffect, useRef, useState, type ReactElement } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { FaYoutube } from "react-icons/fa";
import { useModalStore } from "@/shared/store/modalStore.ts";
import styles from "./youtube-download-sign-in.module.css";

const REQUEST = "download-youtube-sign-in-required";
const RESPONSE = "download-youtube-sign-in-response";
const DISMISSED = "download-youtube-sign-in-dismissed";
const MAIN_WINDOW = { kind: "WebviewWindow", label: "main" } as const;

export function respondToYouTubeDownloadSignIn(requestId: string, proceed: boolean) {
  return emitTo(MAIN_WINDOW, RESPONSE, { request_id: requestId, proceed });
}

export function YouTubeSignInModal({ requestId }: { requestId: string }) {
  const [responding, setResponding] = useState(false);
  const [responseError, setResponseError] = useState(false);

  const respond = async (proceed: boolean) => {
    setResponding(true);
    setResponseError(false);
    try {
      await respondToYouTubeDownloadSignIn(requestId, proceed);
    } catch {
      setResponding(false);
      setResponseError(true);
    }
  };

  return (
    <section
      className={styles.modal}
      role="dialog"
      aria-modal="true"
      aria-labelledby="youtube-download-sign-in-title"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !responding) void respond(false);
      }}
    >
      <div className={styles.icon}><FaYoutube aria-hidden="true" /></div>
      <h2 id="youtube-download-sign-in-title" className={styles.title}>Sign in to YouTube</h2>
      <p>This download uses YouTube for audio. Blacktape needs a signed-in YouTube session to fetch those tracks.</p>
      <p>Continue to sign in on Google. Blacktape keeps the session in its browser profile for future YouTube downloads.</p>
      {responseError && <p role="alert">Could not send your choice. Try again.</p>}
      <div className={styles.actions}>
        <button type="button" className={styles.secondary} onClick={() => void respond(false)} disabled={responding} autoFocus>
          Cancel
        </button>
        <button type="button" className={styles.primary} onClick={() => void respond(true)} disabled={responding}>
          Continue to YouTube
        </button>
      </div>
    </section>
  );
}

export function YouTubeDownloadSignIn() {
  const modalOpen = useModalStore((state) => state.isOpen);
  const [requestId, setRequestId] = useState<string>();
  const displayed = useRef<{ id: string; content: ReactElement } | undefined>(undefined);

  useEffect(() => {
    let mounted = true;
    const closeDisplayed = (id?: string) => {
      const active = displayed.current;
      if (!active || (id !== undefined && id !== active.id)) return;
      displayed.current = undefined;
      const { content, closeModal, stack } = useModalStore.getState();
      if (content === active.content) closeModal();
      else if (stack.some((item) => item.content === active.content)) {
        useModalStore.setState({ stack: stack.filter((item) => item.content !== active.content) });
      }
    };
    const target = { target: MAIN_WINDOW };
    const subscriptions = [
      listen<string>(REQUEST, ({ payload }) => { if (mounted) setRequestId(payload); }, target),
      listen<string>(DISMISSED, ({ payload }) => {
        if (!mounted) return;
        closeDisplayed(payload);
        setRequestId((current) => current === payload ? undefined : current);
      }, target),
    ];
    const settled = Promise.allSettled(subscriptions);
    return () => {
      mounted = false;
      closeDisplayed();
      void settled.then((results) => {
        for (const result of results) if (result.status === "fulfilled") void result.value();
      });
    };
  }, []);

  useEffect(() => {
    const modal = useModalStore.getState();
    if (!requestId || modalOpen || modal.isOpen) return;
    const content = <YouTubeSignInModal requestId={requestId} />;
    displayed.current = { id: requestId, content };
    modal.openModal(content, { dismissible: false });
  }, [modalOpen, requestId]);

  return null;
}
