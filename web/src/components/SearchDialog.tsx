import { useMemo, useState } from "react";
import type { Email, EmailId } from "../types/mail";
import { emailMatches } from "../types/mail";
import { Icon } from "./Icon";
import "./SearchDialog.css";

interface SearchDialogProps {
  emails: Email[];
  onOpen: (id: EmailId) => void;
  onDismiss: () => void;
}

/** Port of views/search_view.rs (680x430, arrows + enter, click to open). */
export function SearchDialog({ emails, onOpen, onDismiss }: SearchDialogProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);

  const results = useMemo(() => emails.filter((e) => emailMatches(e, query)), [emails, query]);
  const clampedIndex = results.length === 0 ? 0 : Math.min(selectedIndex, results.length - 1);

  return (
    <div
      id="search-dialog"
      role="dialog"
      aria-label="Search mail"
      className="nori-search"
      onMouseDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onDismiss();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          if (results.length > 0) setSelectedIndex((i) => (i + 1) % results.length);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          if (results.length > 0)
            setSelectedIndex((i) => (i + results.length - 1) % results.length);
        } else if (e.key === "Enter") {
          const hit = results[clampedIndex];
          if (hit) onOpen(hit.id);
        }
      }}
    >
      <div className="nori-search-header">
        <Icon path="icons/search.svg" size={16} color="var(--nori-muted)" />
        <input
          id="search-query"
          className="nori-search-input"
          value={query}
          placeholder="Search mail"
          autoFocus
          onChange={(e) => {
            setQuery(e.target.value);
            setSelectedIndex(0);
          }}
        />
        <span className="nori-search-esc">Esc</span>
      </div>
      <div id="search-results-scroll" className="nori-search-results">
        {results.map((email, index) => {
          const selected = index === clampedIndex;
          return (
            <div
              key={email.id}
              id={`search-result-${email.id}`}
              role="option"
              aria-selected={selected}
              onClick={() => onOpen(email.id)}
              onMouseEnter={() => setSelectedIndex(index)}
              className={
                selected ? "nori-search-row nori-search-row--selected" : "nori-search-row"
              }
            >
              <div className="nori-search-row-top">
                <span className="nori-search-row-sender">{email.sender}</span>
                <span className="nori-search-row-subject">{email.subject}</span>
                <span className="nori-search-row-time">{email.timestamp}</span>
              </div>
              <div className="nori-search-row-preview">{email.preview}</div>
            </div>
          );
        })}
        {results.length === 0 && <div className="nori-search-empty">No results.</div>}
      </div>
    </div>
  );
}
