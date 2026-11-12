// A workbook from Cash Gear Workspace in the shell (Track S1): the status bar's badge, the panel
// while it opens or waits to reconnect, and the dialogs a save to Workspace can need: a newer
// version saved meanwhile (Save a Copy, Reload), view access (Save a Copy), a session that ended
// (Reconnect, Save to This Device), a workbook deleted there, and a copy Workspace refused. The
// platform's cloud workbook does the talking; nothing here sees a token or a URL.

import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { CgButton, CgProgressBar, CgStatusBadge, CgWindow } from '@cashgear/ui';
import {
  isCloudSaveError,
  type CloudReason,
  type CloudStatus,
  type CloudWorkbook,
  type OpenedFile,
  type SaveTarget,
} from '../platform';
import { cloudTextsFor, type CloudTexts } from './texts';

type SaveDialog =
  | { kind: 'conflict'; by: string | null; at: string | null }
  | { kind: 'viewOnly' }
  | { kind: 'ended' }
  | { kind: 'gone' }
  | { kind: 'copyRefused' }
  | { kind: 'notSaved'; reason: CloudReason };

export interface CloudToolsOptions {
  cloud: CloudWorkbook | undefined;
  /** The workbook's name as the shell shows it. */
  fileName: string | null;
  openFile: (file: OpenedFile) => Promise<void>;
  writeTo: (target: SaveTarget) => Promise<void>;
  saveAs: () => Promise<void>;
}

export interface CloudTools {
  /** The status bar's badge, while the tab has a Workspace workbook. */
  badge: ReactNode | null;
  overlays: ReactNode;
  /**
   * Takes a save's failure when it is a Workspace one, showing what to do about it. False for
   * anything else, which the shell reports as it always has.
   */
  onSaveError: (error: unknown) => boolean;
  texts: CloudTexts;
}

const NO_STATUS: CloudStatus = { kind: 'closed' };

export function useCloudTools({
  cloud,
  fileName,
  openFile,
  writeTo,
  saveAs,
}: CloudToolsOptions): CloudTools {
  const [status, setStatus] = useState<CloudStatus>(() => cloud?.status() ?? NO_STATUS);
  const [dialog, setDialog] = useState<SaveDialog | null>(null);
  // A panel the user closed stays closed until the status moves on.
  const [dismissed, setDismissed] = useState<CloudStatus | null>(null);
  const t = cloudTextsFor(cloud?.language());

  useEffect(() => {
    if (!cloud) return;
    setStatus(cloud.status());
    return cloud.onStatus(setStatus);
  }, [cloud]);

  // Input in the tab keeps the session open, at most one Renew every 5 minutes.
  useEffect(() => {
    if (!cloud) return;
    const touch = () => cloud.touch();
    const options = { capture: true, passive: true } as const;
    window.addEventListener('keydown', touch, options);
    window.addEventListener('pointerdown', touch, options);
    window.addEventListener('wheel', touch, options);
    return () => {
      window.removeEventListener('keydown', touch, options);
      window.removeEventListener('pointerdown', touch, options);
      window.removeEventListener('wheel', touch, options);
    };
  }, [cloud]);

  // The window's title names Workspace's workbook, as ERP data's names its data.
  const titled = 'name' in status && status.kind !== 'opening' ? status.name : null;
  useEffect(() => {
    if (!titled) return;
    const before = document.title;
    document.title = `${titled} – CashGear Sheet`;
    return () => {
      document.title = before;
    };
  }, [titled]);

  const onSaveError = useCallback(
    (error: unknown) => {
      if (!cloud || !isCloudSaveError(error)) return false;
      switch (error.reason) {
        case 'conflict':
          setDialog({
            kind: 'conflict',
            by: error.current?.savedBy ?? null,
            at: error.current?.savedAt ?? null,
          });
          break;
        case 'denied':
          setDialog({ kind: error.copy ? 'copyRefused' : 'viewOnly' });
          break;
        case 'session':
          setDialog({ kind: 'ended' });
          break;
        case 'gone':
          setDialog({ kind: 'gone' });
          break;
        case 'cancelled':
          break;
        default:
          setDialog({ kind: 'notSaved', reason: error.reason });
      }
      return true;
    },
    [cloud],
  );

  if (!cloud) return { badge: null, overlays: null, onSaveError, texts: t };

  const name = 'name' in status && status.name ? status.name : (fileName ?? t.WORKSPACE);
  const close = () => setDialog(null);
  const saveCopy = () => {
    close();
    void writeTo(cloud.copyTarget());
  };
  const reload = () => {
    close();
    cloud
      .reload()
      .then(openFile)
      .catch((error: unknown) => {
        if (!onSaveError(error)) setDialog({ kind: 'notSaved', reason: 'server' });
      });
  };
  const reconnect = () => {
    close();
    cloud.reconnect();
  };
  const toDevice = () => {
    close();
    void saveAs();
  };

  const badgeText = t.badgeText(status);
  const badge =
    badgeText === null ? null : (
      <span className="cgs-cloud-status" dir={t.dir} lang={t.lang} data-testid="cloud-status">
        <CgStatusBadge
          data-testid="cloud-badge"
          type={
            status.kind === 'failed'
              ? 'error'
              : status.kind === 'ended' || (status.kind === 'open' && status.access === 'view')
                ? 'warning'
                : 'info'
          }
          text={badgeText}
        />
        {status.kind === 'ended' && (
          <button
            type="button"
            className="cgs-save-status"
            data-testid="cloud-reconnect"
            onClick={reconnect}
          >
            {t.RECONNECT}
          </button>
        )}
        {status.kind === 'waiting' && (
          <button
            type="button"
            className="cgs-save-status"
            data-testid="cloud-stop-waiting"
            onClick={() => cloud.stopWaiting()}
          >
            {t.STOP_WAITING}
          </button>
        )}
      </span>
    );

  // The panel: opening (with its progress), a launch that opened nothing, waiting to reconnect,
  // and a tab that handed its session on.
  const panelKinds = ['opening', 'failed', 'waiting', 'handedOver'];
  const panelOpen = panelKinds.includes(status.kind) && dismissed !== status;
  const opening = status.kind === 'opening' ? status : null;
  const panelTitle =
    status.kind === 'opening'
      ? t.openingTitle(status.name)
      : status.kind === 'waiting'
        ? t.WAITING_TITLE
        : t.WORKSPACE;
  const panelText =
    status.kind === 'opening'
      ? t.openingText(status.name, status.received, status.total)
      : status.kind === 'failed'
        ? t.failedText(status.reason)
        : status.kind === 'waiting'
          ? t.waitingText(status.name)
          : status.kind === 'handedOver'
            ? t.HANDED_OVER
            : '';

  const action = (testId: string, label: string, onClick: () => void, primary = false) => (
    <CgButton
      key={testId}
      intent={primary ? 'primary' : undefined}
      data-testid={testId}
      onClick={onClick}
    >
      {label}
    </CgButton>
  );

  const dialogTitle = !dialog
    ? ''
    : dialog.kind === 'conflict'
      ? t.CONFLICT_TITLE
      : dialog.kind === 'viewOnly'
        ? t.VIEW_ONLY_TITLE
        : dialog.kind === 'ended'
          ? t.ENDED_TITLE
          : dialog.kind === 'gone'
            ? t.GONE_TITLE
            : dialog.kind === 'copyRefused'
              ? t.COPY_REFUSED_TITLE
              : t.NOT_SAVED_TITLE;
  const dialogText = !dialog
    ? ''
    : dialog.kind === 'conflict'
      ? t.conflictText(name, dialog.by, dialog.at)
      : dialog.kind === 'viewOnly'
        ? t.viewOnlyText(name)
        : dialog.kind === 'ended'
          ? t.endedText(name)
          : dialog.kind === 'gone'
            ? t.goneText(name)
            : dialog.kind === 'copyRefused'
              ? t.COPY_REFUSED_TEXT
              : t.saveFailedText(dialog.reason);
  const dialogActions = !dialog
    ? []
    : dialog.kind === 'conflict'
      ? [
          action('cloud-save-copy', t.SAVE_COPY, saveCopy, true),
          action('cloud-reload', t.RELOAD, reload),
          action('cloud-cancel', t.CANCEL, close),
        ]
      : dialog.kind === 'viewOnly'
        ? [
            action('cloud-save-copy', t.SAVE_COPY, saveCopy, true),
            action('cloud-cancel', t.CANCEL, close),
          ]
        : dialog.kind === 'ended'
          ? [
              action('cloud-dialog-reconnect', t.RECONNECT, reconnect, true),
              action('cloud-save-device', t.SAVE_TO_DEVICE, toDevice),
              action('cloud-cancel', t.CANCEL, close),
            ]
          : dialog.kind === 'gone' ||
              dialog.kind === 'copyRefused' ||
              (dialog.kind === 'notSaved' && dialog.reason === 'tooLarge')
            ? [
                action('cloud-save-device', t.SAVE_TO_DEVICE, toDevice, true),
                action('cloud-cancel', t.CANCEL, close),
              ]
            : [action('cloud-cancel', t.CLOSE, close, true)];

  const overlays = (
    <>
      <CgWindow
        open={panelOpen}
        onOpenChange={(open) => {
          if (!open && status.kind !== 'opening') setDismissed(status);
        }}
        headerText={panelTitle}
        width={440}
        data-testid="cloud-panel"
        renderBody={() => (
          <div className="cgs-dialog-column" dir={t.dir} lang={t.lang}>
            <p className="cgs-dialog-message" data-testid="cloud-panel-text" role="status">
              {panelText}
            </p>
            {opening && (
              <CgProgressBar
                data-testid="cloud-progress"
                value={
                  opening.total ? Math.round((opening.received / opening.total) * 100) : undefined
                }
                max={100}
              />
            )}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            {status.kind === 'waiting' &&
              action('cloud-panel-stop', t.STOP_WAITING, () => cloud.stopWaiting())}
            {status.kind !== 'opening' &&
              action('cloud-panel-close', t.CLOSE, () => setDismissed(status), true)}
          </div>
        )}
      />
      <CgWindow
        open={dialog !== null}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        headerText={dialogTitle}
        width={480}
        data-testid="cloud-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column" dir={t.dir} lang={t.lang}>
            <p className="cgs-dialog-message" data-testid="cloud-dialog-text">
              {dialogText}
            </p>
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            {dialogActions}
          </div>
        )}
      />
    </>
  );

  return { badge, overlays, onSaveError, texts: t };
}
