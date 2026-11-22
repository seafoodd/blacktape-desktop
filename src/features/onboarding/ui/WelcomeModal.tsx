import { pickFolder } from "@/shared/lib/ui/dialog.ts";
import { useOnboardingStore } from "../model/onboardingStore.ts";
import {
  PRIVACY_BULLETS,
  formatFolderDisplay,
  stepSubtitle,
  stepTitle,
} from "../model/welcomeModalHelper.ts";
import styles from "./welcome-modal.module.css";
import { MdClose } from "react-icons/md";

export const WelcomeModal = () => {
  const {
    step,
    libraryDir,
    telemetryEnabled,
    isSaving,
    setLibraryDir,
    setTelemetryEnabled,
    nextStep,
    prevStep,
    finishOnboarding,
    skipOnboarding,
  } = useOnboardingStore();

  const handlePickFolder = async () => {
    const dir = await pickFolder();
    if (dir) {
      setLibraryDir(dir);
    }
  };

  return (
    <div className={styles.container} role="dialog" aria-modal="true">
      <header className={styles.header}>
        <div>
          <h2 className={styles.title}>{stepTitle(step)}</h2>
          <p className={styles.subtitle}>{stepSubtitle(step)}</p>
        </div>
        <button
          className={styles.closeButton}
          onClick={() => skipOnboarding()}
          aria-label="Close setup"
          title="Close setup"
        >
          <MdClose size={18} />
        </button>
      </header>

      <div className={styles.body}>
        {step === 1 && (
          <div>
            <div className={styles.sectionLabel}>Where is your music stored?</div>
            <div className={styles.folderBox}>
              <span
                className={`${styles.folderPath} ${!libraryDir ? styles.folderPathMuted : ""}`}
                title={libraryDir || ""}
              >
                {formatFolderDisplay(libraryDir)}
              </span>
              <button
                type="button"
                className={styles.chooseButton}
                onClick={handlePickFolder}
              >
                Choose…
              </button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div>
            <div className={styles.privacyCard}>
              <p className={styles.privacyText}>
                Blacktape can collect anonymous crash reports and diagnostics so issues can be caught and fixed quickly.
              </p>
              <ul className={styles.bulletList}>
                {PRIVACY_BULLETS.map((bullet, i) => (
                  <li key={i} className={styles.bulletItem}>
                    <span className={styles.bulletCheck}>✓</span>
                    <span>{bullet}</span>
                  </li>
                ))}
              </ul>
            </div>

            <label className={styles.toggleRow}>
              <input
                type="checkbox"
                checked={telemetryEnabled}
                onChange={(e) => setTelemetryEnabled(e.target.checked)}
              />
              <span className={styles.toggleLabel}>
                Send anonymous crash reports & diagnostics
              </span>
            </label>
          </div>
        )}
      </div>

      <footer className={styles.footer}>
        <div className={styles.footerLeft}>
          {step === 1 ? (
            <button
              type="button"
              className={styles.skipButton}
              onClick={() => skipOnboarding()}
            >
              I'll add music later
            </button>
          ) : (
            <button
              type="button"
              className={styles.secondaryButton}
              onClick={prevStep}
              disabled={isSaving}
            >
              ← Back
            </button>
          )}
        </div>

        <div className={styles.footerRight}>
          {step === 1 ? (
            <button
              type="button"
              className={styles.primaryButton}
              onClick={nextStep}
            >
              Continue →
            </button>
          ) : (
            <button
              type="button"
              className={styles.primaryButton}
              onClick={() => finishOnboarding()}
              disabled={isSaving}
            >
              {isSaving ? "Saving…" : "Get Started"}
            </button>
          )}
        </div>
      </footer>
    </div>
  );
};
