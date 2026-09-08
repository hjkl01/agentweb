import { useEffect, useRef } from 'react';
import type { RefObject } from 'react';

type Options = {
  contentVersion: unknown;
  threshold?: number;
};

export function useAutoScroll<T extends HTMLElement>(ref: RefObject<T | null>, { contentVersion, threshold = 80 }: Options) {
  const stickToBottom = useRef(true);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;

    const onScroll = () => {
      const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
      stickToBottom.current = distance <= threshold;
    };

    onScroll();
    element.addEventListener('scroll', onScroll, { passive: true });
    return () => element.removeEventListener('scroll', onScroll);
  }, [ref, threshold]);

  useEffect(() => {
    const element = ref.current;
    if (!element || !stickToBottom.current) return;
    requestAnimationFrame(() => {
      element.scrollTo({ top: element.scrollHeight, behavior: 'smooth' });
    });
  }, [contentVersion, ref]);
}
