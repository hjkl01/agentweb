import { useEffect, useState } from "react";
import type { RefObject } from "react";
import { firstLine } from "../lib/format";
import type { ChatMessage } from "../types";

/** 少于这么多轮的会话一屏就能看完，导航条只添乱。 */
const MIN_TURNS = 6;
/** 滚动位置超过这个比例视作“正在读的那一行”。 */
const READING_LINE = 0.3;

/** 长会话右侧的轮次导航：每轮一个点，点一下跳到那一轮。 */
export function ChatMinimap({
  messages,
  chatRef,
}: {
  messages: ChatMessage[];
  chatRef: RefObject<HTMLElement | null>;
}) {
  const turns = messages.filter((message) => message.role === "user");
  // 依赖 id 串而不是数组本身：流式输出每 token 都会换一次数组，重新测量就白费了。
  const signature = turns.map((turn) => turn.id).join("\n");
  const [activeId, setActiveId] = useState<string>();
  const [show, setShow] = useState(false);

  useEffect(() => {
    const container = chatRef.current;
    const ids = signature ? signature.split("\n") : [];
    if (!container || ids.length < MIN_TURNS) {
      setShow(false);
      return;
    }
    const anchor = (id: string) =>
      container.querySelector<HTMLElement>(`[data-message-id="${id}"]`);
    const update = () => {
      const scrollable = container.scrollHeight - container.clientHeight;
      if (scrollable <= 40) {
        setShow(false);
        return;
      }
      const box = container.getBoundingClientRect();
      const readingLine = container.clientHeight * READING_LINE;
      let current = ids[0];
      for (const id of ids) {
        const element = anchor(id);
        if (element && element.getBoundingClientRect().top - box.top <= readingLine) {
          current = id;
        }
      }
      setActiveId(current);
      setShow(true);
    };
    update();
    container.addEventListener("scroll", update, { passive: true });
    // 窗口尺寸变了 clientHeight 跟着变，靠 resize 事件重新算一次。
    window.addEventListener("resize", update);
    return () => {
      container.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, [chatRef, signature]);

  if (!show) return null;

  const jump = (id: string) => {
    const container = chatRef.current;
    const element = container?.querySelector<HTMLElement>(`[data-message-id="${id}"]`);
    if (!container || !element) return;
    const top =
      element.getBoundingClientRect().top -
      container.getBoundingClientRect().top +
      container.scrollTop;
    setActiveId(id);
    container.scrollTo({
      top: Math.max(0, top - container.clientHeight * READING_LINE),
      behavior: "smooth",
    });
  };

  return (
    <nav className="minimap" aria-label="轮次导航">
      {turns.map((turn, index) => {
        const label = `第 ${index + 1} 轮：${firstLine(turn.content) ?? "（无内容）"}`;
        return (
          <button
            key={turn.id}
            type="button"
            className={`minimap-tick${turn.id === activeId ? " active" : ""}`}
            title={label}
            aria-label={label}
            aria-current={turn.id === activeId ? "true" : undefined}
            onClick={() => jump(turn.id)}
          />
        );
      })}
    </nav>
  );
}