import { useState } from "react";
import type { DraftSeed } from "../types/mail";
import { getNoriBridge } from "../bridge/noriBridge";
import { Button } from "./Button";
import { Icon } from "./Icon";
import "./ComposePane.css";

interface ComposePaneProps {
  seed: DraftSeed;
  onChange: (draft: DraftSeed) => void;
  onClose: () => void;
  onSend: (draft: DraftSeed) => void;
}

function looksLikeAddress(address: string): boolean {
  const trimmed = address.trim();
  if (!trimmed || /\s/.test(trimmed) || trimmed.includes("<") || trimmed.includes(">")) {
    return false;
  }
  const at = trimmed.indexOf("@");
  if (at === -1) return false;
  const domain = trimmed.slice(at + 1);
  return at > 0 && domain.includes(".") && !domain.startsWith(".");
}

/**
 * Port of views/compose_view.rs: a second column beside the mail list,
 * not a card on top of it. Controlled inputs report every keystroke
 * upward so the draft survives hiding (settings), unmounts, and Esc.
 */
export function ComposePane({ seed, onChange, onClose, onSend }: ComposePaneProps) {
  const [error, setError] = useState<string | null>(null);
  const host = getNoriBridge();
  const canSend = host != null && seed.to.trim() !== "";

  const send = () => {
    const recipients = seed.to
      .split(/[,;]/)
      .map((r) => r.trim())
      .filter(Boolean);
    if (recipients.length === 0) {
      setError("add a recipient before sending");
      return;
    }
    const bad = recipients.find((r) => !looksLikeAddress(r));
    if (bad) {
      setError(`${bad} doesn't look like an email address`);
      return;
    }
    setError(null);
    onSend(seed);
  };

  return (
    <div
      id="compose-pane"
      role="dialog"
      aria-label="Compose"
      className="nori-compose-pane"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="nori-compose-pane-header">
        <span className="nori-compose-pane-title">Compose</span>
        <span style={{ flex: 1 }} />
        <div id="compose-close-wrap">
          <Button
            id="compose-close"
            label=""
            dense
            buttonStyle="ghost"
            ariaLabel="Close compose"
            onClick={onClose}
            icon={<Icon path="icons/close.svg" size={14} color="var(--nori-muted)" />}
          />
        </div>
      </div>

      <div className="nori-compose-pane-fields">
        <label className="nori-compose-pane-field">
          <span className="nori-compose-pane-label">To</span>
          <input
            id="compose-to"
            className="nori-compose-pane-input"
            value={seed.to}
            autoFocus
            onChange={(e) => onChange({ ...seed, to: e.target.value })}
          />
        </label>
        <label className="nori-compose-pane-field">
          <span className="nori-compose-pane-label">Subject</span>
          <input
            id="compose-subject"
            className="nori-compose-pane-input"
            value={seed.subject}
            onChange={(e) => onChange({ ...seed, subject: e.target.value })}
          />
        </label>
        <div className="nori-compose-pane-message-wrap">
          <textarea
            id="compose-message"
            className="nori-compose-pane-message"
            value={seed.body}
            placeholder="Message"
            onChange={(e) => onChange({ ...seed, body: e.target.value })}
          />
        </div>
        {error != null && (
          <div id="compose-error" className="nori-compose-pane-error">
            {error}
          </div>
        )}
      </div>

      <div className="nori-compose-pane-footer">
        <Button
          id="compose-attach"
          label="Attach"
          dense
          buttonStyle="ghost"
          disabled
          icon={<Icon path="icons/paperclip.svg" size={13} color="var(--nori-faint)" />}
        />
        <span style={{ flex: 1 }} />
        <Button
          id="compose-send"
          label="Send"
          dense
          buttonStyle="accent"
          disabled={!canSend}
          onClick={send}
          icon={<Icon path="icons/send.svg" size={13} color="var(--nori-faint)" />}
        />
      </div>
    </div>
  );
}
