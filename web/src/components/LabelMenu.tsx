import { useEffect, useRef, useState } from "react";
import { labelChip, LABEL_COLOURS, type Label, type LabelId } from "../state/store";
import type { EmailId } from "../types/mail";
import { Icon } from "./Icon";
import "./LabelMenu.css";

interface LabelMenuProps {
  emailId: EmailId;
  labels: Label[];
  assigned: LabelId[];
  onToggle: (labelId: LabelId) => void;
  onCreate: (name: string) => void;
  onDismiss: () => void;
}

/**
 * Port of the row label menu (mail_app.rs render_label_menu): 200px, flipped
 * to the other side of its row near the window edge, with a "New label" field
 * at the bottom. Clicking outside dismisses it.
 */
export function LabelMenu({
  emailId,
  labels,
  assigned,
  onToggle,
  onCreate,
  onDismiss,
}: LabelMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState("");
  const [flip, setFlip] = useState(false);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onDismiss();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onDismiss();
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  // Flip to the row's left when there is no room on the right.
  useEffect(() => {
    const row = document.getElementById(`email-row-${emailId}`);
    if (!row) return;
    const right = window.innerWidth - row.getBoundingClientRect().right;
    setFlip(right < 216);
  }, [emailId]);

  return (
    <div
      id="label-menu"
      ref={ref}
      role="menu"
      aria-label="Labels"
      className={flip ? "nori-label-menu nori-label-menu--flip" : "nori-label-menu"}
    >
      {labels.length === 0 && <div className="nori-label-menu-empty">No labels yet</div>}
      {labels.map((label) => {
        const chip = labelChip(label.colour);
        const held = assigned.includes(label.id);
        return (
          <button
            key={label.id}
            id={`label-menu-item-${label.id}`}
            type="button"
            role="menuitemcheckbox"
            aria-checked={held}
            className={held ? "nori-label-menu-item nori-label-menu-item--on" : "nori-label-menu-item"}
            onClick={() => onToggle(label.id)}
          >
            <span
              className="nori-label-menu-swatch"
              style={{ background: chip.text, borderColor: chip.border }}
            />
            <span className="nori-label-menu-name">{label.name}</span>
            {held && <Icon path="icons/reset.svg" size={12} color="var(--nori-faint)" />}
          </button>
        );
      })}
      <div className="nori-label-menu-new">
        <input
          id="new-label-input"
          className="nori-label-menu-input"
          value={draft}
          placeholder="New label"
          autoFocus
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") {
              const name = draft.trim();
              if (name !== "") onCreate(name);
              setDraft("");
            } else if (e.key === "Escape") {
              setDraft("");
            }
          }}
        />
        <div className="nori-label-menu-swatches">
          {LABEL_COLOURS.map((colour) => {
            const chip = labelChip(colour);
            return (
              <span
                key={colour}
                className="nori-label-menu-swatch-option"
                style={{ background: chip.text }}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
