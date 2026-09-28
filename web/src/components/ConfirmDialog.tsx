import { Button } from "./Button";
import "./ConfirmDialog.css";

interface ConfirmDialogProps {
  subject: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Port of the archive confirmation (mail_app.rs:2680-2753): a 400px modal on
 * the overlay scrim, Cancel then Archive, Escape cancels.
 */
export function ConfirmDialog({ subject, onConfirm, onCancel }: ConfirmDialogProps) {
  return (
    <div
      id="confirm-archive-layer"
      className="nori-confirm-layer"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        id="confirm-archive-dialog"
        role="dialog"
        aria-label="Archive this message?"
        className="nori-confirm-dialog"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onCancel();
          }
        }}
      >
        <div className="nori-confirm-title">Archive this message?</div>
        <div className="nori-confirm-body">&quot;{subject}&quot; will leave the Inbox for Archive.</div>
        <div className="nori-confirm-actions">
          <Button id="confirm-archive-cancel" label="Cancel" dense buttonStyle="subtle" onClick={onCancel} />
          <Button id="confirm-archive-confirm" label="Archive" dense buttonStyle="accent" onClick={onConfirm} />
        </div>
      </div>
    </div>
  );
}
