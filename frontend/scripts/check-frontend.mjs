// 前端纯逻辑的检查：用 tsc 编译 src/lib/*.ts 后再断言（见 make test）。
import assert from "node:assert/strict";
import {
  diffLines,
  firstLine,
  formatDuration,
  formatToolDuration,
  responseSeconds,
  toolFieldsOfInput,
  toolResultsByCall,
  toolSummary,
} from "../.tmp-test/lib/format.js";
import { familyRows } from "../.tmp-test/lib/sessionFamily.js";
import {
  DEFAULT_PREFERENCES,
  clampChatContentWidth,
  clampChatFontSize,
  parsePreferences,
} from "../.tmp-test/lib/preferences.js";
import { ARCHIVED_KEY, groupSessions } from "../.tmp-test/lib/sessionGroups.js";
import {
  diffGapLabel,
  parseUnifiedPatch,
  unifiedDiffLines,
} from "../.tmp-test/lib/patch.js";
import { settlePending } from "../.tmp-test/lib/pendingMessages.js";
import {
  turnWrittenFiles,
  workspaceRelative,
} from "../.tmp-test/lib/turnFiles.js";

const session = (id, native, forkFrom) => ({
  id,
  native_session_id: native ?? null,
  fork_from_native_session_id: forkFrom ?? null,
});
const rows = (list) =>
  familyRows(list).map((row) => `${row.session.id}:${row.depth}`);

// 父会话在后也有可能：子会话先出现在列表里，排序后仍要父在前、逐层缩进。
assert.deepEqual(
  rows([
    session("c", "nc", "nb"),
    session("b", "nb", "na"),
    session("a", "na"),
  ]),
  ["a:0", "b:1", "c:2"],
);
// 无关会话保持原顺序，分支紧跟父会话。
assert.deepEqual(
  rows([session("a", "na"), session("x", "nx"), session("b", "nb", "na")]),
  ["a:0", "b:1", "x:0"],
);
// 分叉出的会话自身还没写文件（没有原生 id）时，仍属于它的父会话。
assert.deepEqual(rows([session("a", "na"), session("b", null, "na")]), [
  "a:0",
  "b:1",
]);
// 父会话不在这一批（例如父会话已归档、子会话还在项目组）时不能丢行。
assert.deepEqual(rows([session("b", "nb", "na")]), ["b:0"]);
// 自引用与互相引用都不能死循环，也不能丢行。
assert.deepEqual(rows([session("a", "na", "na")]), ["a:0"]);
assert.deepEqual(
  rows([session("a", "na", "nb"), session("b", "nb", "na")]).sort(),
  ["a:0", "b:1"],
);
assert.deepEqual(familyRows([]), []);

console.log("sessionFamily: ok");

// 偏好存档：坏数据与非布尔值一律退回默认。
assert.deepEqual(parsePreferences(null), DEFAULT_PREFERENCES);
assert.deepEqual(parsePreferences("not json"), DEFAULT_PREFERENCES);
assert.deepEqual(parsePreferences("[1,2]"), DEFAULT_PREFERENCES);
assert.deepEqual(parsePreferences('{"completionSound":"yes"}'), {
  ...DEFAULT_PREFERENCES,
  completionSound: false,
});
assert.equal(
  parsePreferences('{"completionSound":true}').completionSound,
  true,
);
assert.equal(
  parsePreferences('{"completionSound":false}').completionSound,
  false,
);
// 数值要夹在合法区间，非数值一律退回默认值。
assert.equal(clampChatFontSize(11), 12);
assert.equal(clampChatFontSize(999), 20);
assert.equal(clampChatContentWidth(100), 640);
assert.equal(clampChatContentWidth(5000), 1600);
assert.equal(clampChatContentWidth(1000.4), 1000);
assert.equal(clampChatFontSize("14"), 13);
assert.equal(clampChatContentWidth(null), 900);
assert.deepEqual(parsePreferences('{"chatFontSize":99,"sendMode":"meta"}'), {
  ...DEFAULT_PREFERENCES,
  chatFontSize: 20,
});
assert.equal(
  parsePreferences('{"sendMode":"ctrlEnter"}').sendMode,
  "ctrlEnter",
);
assert.equal(
  parsePreferences('{"thinkingExpanded":true}').thinkingExpanded,
  true,
);
// 引用选区是已有功能，旧存档没有这个键时保持开启。
assert.equal(parsePreferences('{"completionSound":true}').quoteSelection, true);
assert.equal(
  parsePreferences('{"quoteSelection":false}').quoteSelection,
  false,
);
console.log("preferences: ok");

// 会话分组：置顶最前、归档最后、项目按首次出现顺序、worktree 归并回主仓库。
const grouped = (id, extra = {}) => ({
  id,
  workspace: `/w/${id}`,
  is_pinned: false,
  archived_at: null,
  ...extra,
});
const groupNames = groupSessions([
  grouped("a", { workspace: "/w/repo" }),
  grouped("b", { workspace: "/w/other" }),
  grouped("c", { workspace: "/w/repo", is_pinned: true }),
  grouped("d", { workspace: "/w/repo", archived_at: "2026-01-01T00:00:00Z" }),
  grouped("e", { workspace: "/w/repo-worktrees/feature", updated: true }),
]).map((group) => `${group.key}:${group.sessions.map((s) => s.id).join(",")}`);
assert.deepEqual(groupNames, [
  "__pinned__:c",
  "/w/repo:a,e",
  "/w/other:b",
  `${ARCHIVED_KEY}:d`,
]);
assert.deepEqual(groupSessions([]), []);
// 每个会话都必须出现在某个分组里，否则侧栏会漏掉会话。
const all = groupSessions([
  grouped("a", { workspace: "/w/repo" }),
  grouped("b", { workspace: "/w/repo", is_pinned: true }),
  grouped("c", { archived_at: "2026-01-01T00:00:00Z" }),
]);
assert.equal(
  all.reduce((total, group) => total + group.sessions.length, 0),
  3,
);
console.log("sessionGroups: ok");

// 耗时展示：不足 1 秒按毫秒，1 分钟以上拆成分秒，坏值不显示。
assert.equal(formatDuration(0), "0ms");
assert.equal(formatDuration(850), "850ms");
assert.equal(formatDuration(4200), "4.2s");
assert.equal(formatDuration(45000), "45s");
assert.equal(formatDuration(95000), "1m35s");
assert.equal(formatDuration(undefined), undefined);
assert.equal(formatDuration(Number.NaN), undefined);
assert.equal(formatDuration(-1), undefined);

// 工具耗时：跟 pi CLI 的 Took 一致，太快（<100ms）就不显示。
assert.equal(formatToolDuration(100), "0.1s");
assert.equal(formatToolDuration(2400), "2.4s");
assert.equal(formatToolDuration(59400), "59.4s");
assert.equal(formatToolDuration(65000), "1m 5s");
assert.equal(formatToolDuration(3725000), "1h 2m 5s");
assert.equal(formatToolDuration(99), undefined);
assert.equal(formatToolDuration(undefined), undefined);

// 参数表：只有「一排标量」才排成两列，出现嵌套对象/空值就整体退回 JSON（pi-web 的规则）。
assert.deepEqual(toolFieldsOfInput({ command: "ls", timeout: 5 }), [
  { key: "command", value: "ls", multiline: false },
  { key: "timeout", value: "5", multiline: false },
]);
assert.equal(toolFieldsOfInput({ content: "a\nb" })?.[0].multiline, true);
assert.equal(toolFieldsOfInput({ a: { b: 1 } }), undefined);
assert.equal(toolFieldsOfInput({ a: "" }), undefined);
assert.equal(toolFieldsOfInput({}), undefined);
assert.equal(toolFieldsOfInput("ls"), undefined);
console.log("format: ok");

// 工具调用摘要：取首个关键词参数，压掉换行并截断。
assert.equal(toolSummary({ command: "npm test" }), "npm test");
assert.equal(toolSummary({ path: "src/a.ts" }), "src/a.ts");
assert.equal(toolSummary({ command: "a\n  b" }), "a b");
assert.equal(toolSummary({ other: "x" }), undefined);
assert.equal(toolSummary({}), undefined);
assert.equal(toolSummary(null), undefined);
assert.equal(toolSummary({ command: "x".repeat(500) }).length, 120);
console.log("toolSummary: ok");

// 编辑工具的 diff 片段：区分新增/删除/上下文/空档行。
assert.deepEqual(diffLines("@@ -1 +1 @@\n-old\n+new\n keep\n   ..."), [
  { kind: "hunk", text: "@@ -1 +1 @@" },
  { kind: "remove", text: "-old" },
  { kind: "add", text: "+new" },
  { kind: "context", text: " keep" },
  { kind: "gap", text: "   ..." },
]);
assert.equal(diffLines("").length, 1);
console.log("diffLines: ok");

// 执行过程分组：思考与工具调用合成一段，工具结果按 tool_call_id 挂回那次调用。
const thinking = (text) => ({ type: "thinking", text });
const call = (id, name, args) => ({
  type: "toolCall",
  tool_call_id: id,
  name,
  arguments: args,
});
const assistant = (id, parts, extra = {}) => ({
  id,
  role: "assistant",
  content: "",
  parts,
  ...extra,
});
const result_ = (id, callId, extra = {}) => ({
  id,
  role: "toolResult",
  content: "输出",
  tool_call_id: callId,
  ...extra,
});
// 工具结果按 tool_call_id 挂回可见的调用；孤立结果（调用被分页截断）不配，由 ChatPanel 单独成卡。
const byCall = toolResultsByCall([
  assistant("a", [call("c1", "bash", {})]),
  result_("r1", "c1", { duration_ms: 12 }),
  result_("r2", "gone", { tool_name: "bash" }),
]);
assert.equal(byCall.get("c1").id, "r1");
assert.equal(byCall.has("gone"), false);
assert.equal(toolResultsByCall([result_("r1", "c1")]).size, 0);
assert.equal(toolResultsByCall([assistant("a", [call("c1", "bash", {})])]).size, 0);
// 思考耗时：助手消息跟上一条消息的时间差，整秒；同一秒或没有时间戳就不显示。
const seconds = responseSeconds([
  { id: "u1", role: "user", content: "", created_at: "2026-01-01T00:00:00Z" },
  { id: "a1", role: "assistant", content: "", created_at: "2026-01-01T00:00:04.600Z" },
  { id: "a2", role: "assistant", content: "", created_at: "2026-01-01T00:00:04.600Z" },
]);
assert.equal(seconds.get("a1"), 5);
assert.equal(seconds.has("a2"), false);
assert.equal(responseSeconds([{ id: "a1", role: "assistant", content: "" }]).size, 0);
console.log("activity: ok");
assert.equal(firstLine("\n\n  第一行  \n第二行"), "第一行");
assert.equal(firstLine("   "), undefined);
// patch: 统一 diff 解析成分栏行（左右行号 + 空格子）和单列行
{
  const patch = [
    "--- a/x.ts",
    "+++ b/x.ts",
    "@@ -1,4 +1,5 @@",
    " const a = 1;",
    "-const b = 2;",
    "+const b = 3;",
    "+const c = 4;",
    "@@ -10,2 +11,2 @@",
    " tail(1);",
    "-tail(2);",
    "+tail(3);",
  ].join("\n");
  const files = parseUnifiedPatch(patch);
  assert.equal(files.length, 1);
  assert.equal(files[0].newPath, "b/x.ts");
  assert.deepEqual(
    files[0].rows.map((row) => row.type),
    ["hunk", "line", "line", "line", "gap", "hunk", "line", "line"],
  );
  // 两个 hunk 之间没显示的老文件行数：10 - (1 + 4)
  assert.equal(files[0].rows[4].count, 5);
  assert.deepEqual(files[0].rows[2].left, {
    lineNo: 2,
    type: "removed",
    text: "const b = 2;",
  });
  assert.deepEqual(files[0].rows[2].right, {
    lineNo: 2,
    type: "added",
    text: "const b = 3;",
  });
  // 只有新增的那一行，左侧补空格子
  assert.equal(files[0].rows[3].left.type, "empty");
  assert.equal(files[0].rows[3].right.type, "added");
  // hunk 里的 `--- ` / `+++ ` 是内容，不能当文件头
  const tricky = parseUnifiedPatch(
    ["--- a/f", "+++ b/f", "@@ -1,2 +1,2 @@", "--- x", "+++ y"].join("\n"),
  );
  assert.equal(tricky.length, 1);
  assert.equal(tricky[0].rows.at(-1).left.text, "-- x");
  // 解析不动就交回 null，让界面去显示原文
  assert.equal(parseUnifiedPatch("hello"), null);
  // 单列：删的行压在增的行上面，hunk 头不进正文
  assert.deepEqual(
    unifiedDiffLines(files[0]).map((line) => line.type),
    [
      "context",
      "removed",
      "added",
      "added",
      "gap",
      "context",
      "removed",
      "added",
    ],
  );
  assert.equal(diffGapLabel(5), "5 行未改动");
}
console.log("patch: ok");

// 本地回显的用户消息：转录里出现了就撤掉，否则留着（否则追加的任务会从页面消失）。
assert.deepEqual(settlePending(["还没有的"], []), ["还没有的"]);
assert.deepEqual(
  settlePending(["已进转录"], [{ role: "user", content: "已进转录" }]),
  [],
);
assert.deepEqual(
  settlePending(["已进转录"], [{ role: "assistant", content: "已进转录" }]),
  ["已进转录"],
);
assert.deepEqual(
  settlePending([" 带空白 "], [{ role: "user", content: "带空白\n" }]),
  [],
);
assert.deepEqual(settlePending(["a", "b"], [{ role: "user", content: "b" }]), [
  "a",
]);
const unchanged = ["a"];
assert.equal(settlePending(unchanged, []), unchanged);
console.log("pendingMessages: ok");

// turnFiles：每轮写入文件只认工具调用的参数，不扫回复正文。
assert.equal(
  workspaceRelative("/home/ljl/proj", "/home/ljl/proj/src/a.ts"),
  "src/a.ts",
);
assert.equal(workspaceRelative("/home/ljl/proj", "/etc/passwd"), undefined);
assert.equal(workspaceRelative("/home/ljl/proj/", "src/b.ts"), "src/b.ts");
const writeCall = (id, name, args) => ({
  type: "toolCall",
  name,
  arguments: args,
  tool_call_id: id,
});
const callResult = (callId, extra = {}) => ({
  id: `r-${callId}`,
  role: "toolResult",
  content: "",
  tool_call_id: callId,
  ...extra,
});
const turn = {
  id: "a1",
  role: "assistant",
  content: "",
  parts: [
    writeCall("c1", "write", { path: "/home/ljl/proj/src/a.ts" }),
    writeCall("c2", "edit", { file_path: "/home/ljl/proj/src/a.ts" }),
    writeCall("c3", "bash", { command: "touch x" }),
    writeCall("c4", "write", { path: "/etc/passwd" }),
  ],
};
const results = new Map(
  ["c1", "c2", "c3", "c4"].map((id) => [id, callResult(id)]),
);
// 同一文件写两次只列一个；工作区外的路径不列；非写文件的工具不列。
assert.deepEqual(
  turnWrittenFiles([turn], results, "/home/ljl/proj")
    .get("a1")
    .map((file) => file.path),
  ["src/a.ts"],
);
// 结果缺失（调用被分页截断）或失败时都不列。
assert.equal(
  turnWrittenFiles([turn], new Map(), "/home/ljl/proj").size,
  0,
);
assert.deepEqual(
  turnWrittenFiles(
    [turn],
    new Map([...results, ["c1", callResult("c1", { is_error: true })]]),
    "/home/ljl/proj",
  )
    .get("a1")
    .map((file) => file.path),
  ["src/a.ts"],
);
// apply_patch 的路径只能从补丁文本取，删除的文件不算写入。
const patched = turnWrittenFiles(
  [
    {
      id: "a2",
      role: "assistant",
      content: "",
      parts: [writeCall("p1", "apply_patch", { patch: "*** Begin Patch" })],
    },
  ],
  new Map([
    [
      "p1",
      callResult("p1", {
        patch:
          "*** Begin Patch\n*** Add File: /home/ljl/proj/notes.md\n+x\n*** Update File: /home/ljl/proj/src/a.ts\n*** Delete File: /home/ljl/proj/old.md\n*** End Patch",
      }),
    ],
  ]),
  "/home/ljl/proj",
);
assert.deepEqual(
  patched.get("a2").map((file) => file.path),
  ["notes.md", "src/a.ts"],
);
console.log("turnFiles: ok");
