import { pickFolder } from "@/shared/lib/ui/dialog.ts";
import { useOnboardingStore } from "../model/onboardingStore.ts";
import {
  PRIVACY_BULLETS,
  formatFolderDisplay,
  stepBadge,
  stepSubtitle,
  stepTitle,
} from "../model/welcomeModalHelper.ts";
import styles from "./welcome-modal.module.css";
import { MdFolder, MdSecurity } from "react-icons/md";

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
        <span className={styles.stepBadge}>{stepBadge(step)}</span>
        <h2 className={styles.title}>{stepTitle(step)}</h2>
        <p className={styles.subtitle}>{stepSubtitle(step)}</p>
      </header>

      <div className={styles.body}>
        {step === 1 && (
          <>
            <p className={styles.stepPrompt}>
              Choose where your local music files are located. Blacktape will automatically index your artists, albums, and tracks.
            </p>

            <div className={styles.folderBox}>
              <div className={styles.folderInfo}>
                <MdFolder className={styles.folderIcon} />
                <div className={styles.folderDetails}>
                  <span className={styles.folderLabel}>Music Folder</span>
                  <span
                    className={`${styles.folderPath} ${!libraryDir ? styles.folderPathMuted : ""}`}
                    title={libraryDir || ""}
                  >
                    {formatFolderDisplay(libraryDir)}
                  </span>
                </div>
              </div>
              <button
                type="button"
                className={styles.chooseButton}
                onClick={handlePickFolder}
              >
                Browse…
              </button>
            </div>

            <p className={styles.hintText}>
              You can change this or add more folders later in Settings.
            </p>
          </>
        )}

        {step === 2 && (
          <>
            <div className={styles.privacyCard}>
              <div className={styles.privacyHeading}>
                <MdSecurity className={styles.privacyHeadingIcon} />
                <span>Your privacy is protected by design</span>
              </div>
              <ul className={styles.bulletList}>
                {PRIVACY_BULLETS.map((bullet, i) => (
                  <li key={i} className={styles.bulletItem}>
                    <span className={styles.bulletCheck}>✓</span>
                    <span>{bullet}</span>
                  </li>
                ))}
              </ul>
            </div>

            <label className={styles.toggleCard}>
              <div className={styles.toggleTextGroup}>
                <span className={styles.toggleTitle}>
                  Send anonymous crash reports & diagnostics
                </span>
                <span className={styles.toggleDescription}>
                  Enables automatic diagnostic logs when playback or downloads fail.
                </span>
              </div>
              <input
                type="checkbox"
                checked={telemetryEnabled}
                onChange={(e) => setTelemetryEnabled(e.target.checked)}
              />
            </label>
          </>
        )}
      </div>

      <footer className={styles.footer}>
        <div className={styles.footerLeft}>
          {step === 1 ? (
            <button
              type="button"
              className={styles.skipButton}
              onClick={nextStep}
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
