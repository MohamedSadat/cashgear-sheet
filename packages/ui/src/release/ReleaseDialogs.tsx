import { useState } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type { NativeInfo } from '../platform';
import type { UpdatesState } from './useUpdates';

/** Written next to the page by `pnpm build` (scripts/third-party-notices.mjs). */
const NOTICES_URL = '/third-party-notices.txt';

/** The version, where it runs, and the open-source software it is built with. */
export function AboutDialog({
  version,
  native,
  onClose,
}: {
  version: string;
  native: NativeInfo | null;
  onClose(): void;
}) {
  const [notices, setNotices] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const showNotices = async () => {
    setLoading(true);
    try {
      const response = await fetch(NOTICES_URL);
      setNotices(
        response.ok
          ? await response.text()
          : 'The notices are written by a release build (pnpm build), and this one has none.',
      );
    } catch (error) {
      setNotices('The notices could not be read: ' + String(error));
    } finally {
      setLoading(false);
    }
  };
  return (
    <CgWindow
      open
      headerText="About CashGear Sheet"
      width={notices === null ? 420 : 720}
      data-testid="about-dialog"
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      renderBody={() => (
        <div className="cgs-about">
          <p data-testid="about-version">
            CashGear Sheet {native?.version ?? version}
            {native ? ` for ${native.os === 'windows' ? 'Windows' : native.os}` : ' on the web'}
          </p>
          <p className="cgs-about-muted">
            Free to use. Workbooks stay on this device: nothing is sent anywhere.
          </p>
          {notices === null ? (
            <CgButton
              data-testid="about-show-notices"
              disabled={loading}
              onClick={() => void showNotices()}
            >
              Third-party notices
            </CgButton>
          ) : (
            <pre className="cgs-about-notices" data-testid="about-notices" tabIndex={0}>
              {notices}
            </pre>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="about-close" onClick={onClose}>
            Close
          </CgButton>
        </div>
      )}
    />
  );
}

/** A newer version: what it is, and installing it once the workbook is safe. */
export function UpdateDialog({
  updates,
  dirty,
  save,
  onClose,
}: {
  updates: UpdatesState;
  /** The workbook has changes that aren't saved. */
  dirty: boolean;
  /** Saves the workbook; true when nothing is left unsaved. */
  save(): Promise<boolean>;
  onClose(): void;
}) {
  const [saving, setSaving] = useState(false);
  const { available, installing, error } = updates;
  if (!available) return null;
  const install = async () => {
    if (dirty) {
      setSaving(true);
      const saved = await save().finally(() => setSaving(false));
      if (!saved) return;
    }
    await updates.install();
  };
  const percent = Math.round((installing?.fraction ?? 0) * 100);
  return (
    <CgWindow
      open
      headerText="Update CashGear Sheet"
      width={440}
      data-testid="update-dialog"
      onOpenChange={(next) => {
        if (!next && !installing) onClose();
      }}
      renderBody={() => (
        <div className="cgs-about">
          <p>
            Version <strong>{available.version}</strong> is ready. Installing it closes CashGear
            Sheet and opens the new version.
          </p>
          {available.notes && <p className="cgs-about-muted cgs-update-notes">{available.notes}</p>}
          {installing && (
            <div
              className="cgs-progress"
              role="progressbar"
              aria-label="Download progress"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent}
            >
              <div className="cgs-progress-bar" style={{ width: `${percent}%` }} />
            </div>
          )}
          {error && (
            <p role="alert" className="cgs-about-warning" data-testid="update-error">
              The update failed: {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="update-install"
            disabled={!!installing || saving}
            onClick={() => void install()}
          >
            {dirty ? 'Save and install' : 'Install and restart'}
          </CgButton>
          {!installing && <CgButton onClick={onClose}>Later</CgButton>}
        </div>
      )}
    />
  );
}
