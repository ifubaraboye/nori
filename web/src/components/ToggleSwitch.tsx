import "./ToggleSwitch.css";

interface ToggleSwitchProps {
  id: string;
  label: string;
  on: boolean;
  onToggle: () => void;
}

/**
 * Port of components/toggle_switch.rs: 30x18 track, 14px knob,
 * accent track parked right when on, surface parked left when off.
 * Operable by pointer, Enter, and Space; exposed as a switch.
 */
export function ToggleSwitch({ id, label, on, onToggle }: ToggleSwitchProps) {
  return (
    <div
      id={id}
      role="switch"
      aria-checked={on}
      aria-label={label}
      tabIndex={0}
      className={on ? "nori-toggle nori-toggle--on" : "nori-toggle"}
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      onKeyDown={(e) => {
        if ((e.key === "Enter" || e.key === " ") && !e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          e.stopPropagation();
          onToggle();
        }
      }}
    >
      <div className="nori-toggle-knob" />
    </div>
  );
}
