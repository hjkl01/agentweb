import { useCallback, useEffect, useRef, useState } from "react";
import {
  DEFAULT_PREFERENCES,
  type Preferences,
  parsePreferences,
} from "../lib/preferences";

const STORAGE_KEY = "agentweb.preferences";

/** 一轮回复结束时响两声，用 Web Audio 合成，不引入音频资源。 */
function playCompletionSound() {
  const AudioContextClass =
    window.AudioContext ||
    (window as unknown as { webkitAudioContext?: typeof AudioContext })
      .webkitAudioContext;
  if (!AudioContextClass) return;
  const context = new AudioContextClass();
  const start = context.currentTime;
  for (const [index, frequency] of [880, 1174].entries()) {
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    gain.gain.setValueAtTime(0.0001, start + index * 0.12);
    gain.gain.exponentialRampToValueAtTime(0.08, start + index * 0.12 + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + index * 0.12 + 0.1);
    oscillator.connect(gain).connect(context.destination);
    oscillator.start(start + index * 0.12);
    oscillator.stop(start + index * 0.12 + 0.11);
  }
  window.setTimeout(() => void context.close(), 600);
}

export function usePreferences() {
  const [preferences, setPreferences] = useState<Preferences>(() =>
    parsePreferences(localStorage.getItem(STORAGE_KEY)),
  );

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  }, [preferences]);

  // 字号与列宽走 CSS 变量：样式表里的默认值和这里一致，JS 没跑起来时布局不变。
  useEffect(() => {
    const root = document.documentElement.style;
    root.setProperty("--chat-font-size", `${preferences.chatFontSize}px`);
    root.setProperty(
      "--chat-content-width",
      `${preferences.chatContentWidth}px`,
    );
  }, [preferences.chatFontSize, preferences.chatContentWidth]);

  const update = useCallback((patch: Partial<Preferences>) => {
    setPreferences((current) =>
      parsePreferences(JSON.stringify({ ...current, ...patch })),
    );
  }, []);

  return { preferences, update, testSound: playCompletionSound };
}

/**
 * 任意会话从运行态回到空闲都提醒一声：用户很可能已经切到别的会话或标签页，
 * 所以看的是整个列表的运行集合，而不是当前会话。
 */
export function useCompletionSound(enabled: boolean, runningIds: string) {
  const previous = useRef<Set<string>>(new Set());
  useEffect(() => {
    const current = new Set(runningIds ? runningIds.split(",") : []);
    const finished = [...previous.current].some((id) => !current.has(id));
    if (enabled && finished) playCompletionSound();
    previous.current = current;
  }, [enabled, runningIds]);
}

export { DEFAULT_PREFERENCES };
