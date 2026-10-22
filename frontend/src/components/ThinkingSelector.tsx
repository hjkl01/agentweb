import { Brain, Check, ChevronDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { THINKING_LEVELS } from "../types";

type Props = {
  value?: string;
  disabled?: boolean;
  onChange: (level?: string) => void;
};

const label = (value?: string) =>
  THINKING_LEVELS.find((level) => level.value === value)?.label;

export function ThinkingSelector({ value, disabled, onChange }: Props) {
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

  const choose = (level?: string) => {
    onChange(level);
    setOpen(false);
  };

  return (
    <div className="model-selector-wrap" ref={ref}>
      <button
        className={`model-selector ${open ? "open" : ""}`}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        aria-label="选择推理等级"
        aria-expanded={open}
      >
        <span className="model-selector-icon">
          <Brain size={14} />
        </span>
        <span className="model-selector-info">
          <small>THINKING</small>
          <strong>{label(value) || "默认"}</strong>
          {value && <em>--thinking {value}</em>}
        </span>
        <ChevronDown size={14} className="model-chevron" />
      </button>
      {open && (
        <div className="model-menu">
          <button
            className={!value ? "selected" : ""}
            onClick={() => choose(undefined)}
          >
            <span>
              <strong>默认</strong>
              <small>使用 Agent 自身配置的推理等级</small>
            </span>
            {!value && <Check size={15} />}
          </button>
          {THINKING_LEVELS.map((level) => (
            <button
              key={level.value}
              className={level.value === value ? "selected" : ""}
              onClick={() => choose(level.value)}
            >
              <span>
                <strong>{level.label}</strong>
                <small>{level.value}</small>
              </span>
              {level.value === value && <Check size={15} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
