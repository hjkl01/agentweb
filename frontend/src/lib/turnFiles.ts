import type { ChatMessage } from "../types";

/** 会写文件的工具。apply_patch 的路径不在参数里，只能从补丁文本取。 */
const WRITE_TOOLS = new Set(["write", "edit", "apply_patch", "multi_edit"]);

export type WrittenFile = {
  /** 工作区相对路径，可以直接交给文件预览接口。 */
  path: string;
  callId: string;
};

function inputPath(args: unknown): string | undefined {
  if (!args || typeof args !== "object") return undefined;
  const { file_path: filePath, path } = args as Record<string, unknown>;
  const value = typeof filePath === "string" ? filePath : path;
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

/** 补丁文本里被新增或修改（不是删除）的文件。 */
function patchPaths(patch: string): string[] {
  const paths: string[] = [];
  for (const line of patch.split("\n")) {
    const match = line.match(/^\*\*\* (?:Add|Update) File: (.+)$/);
    if (match) paths.push(match[1].trim());
  }
  return paths;
}

/** 工具调用的绝对路径转成工作区相对路径；工作区之外的文件返回 undefined。 */
export function workspaceRelative(
  workspace: string,
  path: string,
): string | undefined {
  const clean = path.replace(/^\.\//, "");
  if (!clean.startsWith("/")) return clean;
  const root = workspace.replace(/\/+$/, "");
  return clean.startsWith(`${root}/`) ? clean.slice(root.length + 1) : undefined;
}

/** 每轮助手回复写入过的文件，供消息下方排成可点开的标签。
 *  只看工具调用的参数，不扫回复正文：正文里的路径不是这轮真的写成功的东西。 */
export function turnWrittenFiles(
  messages: ChatMessage[],
  results: Map<string, ChatMessage>,
  workspace: string,
): Map<string, WrittenFile[]> {
  const turns = new Map<string, WrittenFile[]>();
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    const seen = new Set<string>();
    const files: WrittenFile[] = [];
    for (const part of message.parts ?? []) {
      if (part.type !== "toolCall" || !part.tool_call_id) continue;
      if (!part.name || !WRITE_TOOLS.has(part.name)) continue;
      // 结果缺失或失败时不列：那次调用可能根本没写成。
      const result = results.get(part.tool_call_id);
      if (!result || result.is_error) continue;
      const patch = result.patch ?? result.diff;
      const candidates =
        part.name === "apply_patch" || part.name === "multi_edit"
          ? patch
            ? patchPaths(patch)
            : []
          : [inputPath(part.arguments)].filter((item) => !!item);
      for (const candidate of candidates as string[]) {
        const path = workspaceRelative(workspace, candidate);
        if (!path || seen.has(path)) continue;
        seen.add(path);
        files.push({ path, callId: part.tool_call_id });
      }
    }
    if (files.length > 0) turns.set(message.id, files);
  }
  return turns;
}