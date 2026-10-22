import { Check, ChevronDown, Wrench } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { TOOL_PRESETS } from "../types";

type Props = {
  value?: string;
  disabled?: boolean;
  onChange: (preset?: string) => void;
};

const preset = (value?: string) =>
  TOOL_PRESETS.find((entry) => entry.value === value);

export function ToolPresetSelector({ value, disabled, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const choose = (next?: string) => {
    onChange(next);
    setOpen(false);
  };

  return (
    <div className="model-selector-wrap" ref={ref}>
      <button
        className={`model-selector ${open ? "open" : ""}`}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        aria-label="选择工具权限"
        aria-expanded={open}
      >
        <span className="model-selector-icon">
          <Wrench size={14} />
        </span>
        <span className="model-selector-info">
          <small>TOOLS</small>
          <strong>{preset(value)?.label || "默认"}</strong>
          {preset(value) && <em>{preset(value)?.args}</em>}
        </span>
        <ChevronDown size={14} className="model-chevron" />
      </button>
      {open && (
        <div className="model-menu">
          {TOOL_PRESETS.map((entry) => (
            <button
              key={entry.value}
              className={entry.value === (value || "default") ? "selected" : ""}
              onClick={() =>
                choose(entry.value === "default" ? undefined : entry.value)
              }
            >
              <span>
                <strong>{entry.label}</strong>
                <small>{entry.hint}</small>
              </span>
              {entry.value === (value || "default") && <Check size={15} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
