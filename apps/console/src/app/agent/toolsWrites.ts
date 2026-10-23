// The GATED writes outside a character package: the operator's marks on a Review line, and the
// sandbox session's start and stop.
import type { FeedbackTag, SandboxStartParams } from "@shadowrunr/contract";
import { argInt, argStr, fail, int, obj, optStr, str, type ToolSpec } from "./tool";

const TAGS: readonly FeedbackTag[] = ["landed", "flat", "too_mean", "broke_character", "off_tone", "great_roast", "repetitive", "wrong_tag"];

export const WRITE_TOOLS: readonly ToolSpec[] = [
  {
    name: "leave_feedback",
    title: "Mark a Review line",
    description: "Saves the operator's marks on one session line (review.feedback.set): tags and an optional note; the latest marks win, no tags and no note clears the line. Returns the saved row.",
    cls: "GATED",
    input: obj(
      {
        session_id: str("The sandbox session id."),
        line: int("The 1-based transcript line.", 1),
        tags: { type: "array", items: { type: "string", enum: TAGS }, description: "Feedback tags; [] with no note clears." },
        note: str("A short note on the line.", { maxLength: 2000 }),
      },
      ["session_id", "line", "tags"],
    ),
    run: async (input, ctx) => {
      const tags = input["tags"];
      if (!Array.isArray(tags) || !tags.every((t): t is FeedbackTag => TAGS.includes(t))) fail("invalid_params", `tags must be drawn from ${TAGS.join(", ")}`);
      const line = argInt(input, "line", 0, 1, Number.MAX_SAFE_INTEGER);
      if (line < 1) fail("invalid_params", `"line" is required`);
      const note = optStr(input, "note");
      const r = await ctx.write("review.feedback.set", { session_id: argStr(input, "session_id"), line, tags, ...(note === undefined ? {} : { note }) });
      return { saved: r };
    },
  },
  {
    name: "start_sandbox",
    title: "Start a sandbox session",
    description: "Starts a wall-clock sandbox session of 5, 10 or 30 minutes on the daemon's engine, judged when it ends; optional seed and tone. Returns the session id and stage URL. Watch with query sandbox.status.",
    cls: "GATED",
    input: obj(
      {
        minutes: { type: "integer", enum: [5, 10, 30], description: "5, 10 or 30." },
        seed: int("The simulated room's seed.", 0),
        tone: str("A tone variant; the character's active tone when omitted."),
      },
      ["minutes"],
    ),
    run: async (input, ctx) => {
      const minutes = input["minutes"];
      if (minutes !== 5 && minutes !== 10 && minutes !== 30) fail("invalid_params", "minutes must be 5, 10 or 30");
      const seed = input["seed"];
      if (seed !== undefined && (typeof seed !== "number" || !Number.isInteger(seed) || seed < 0)) fail("invalid_params", "seed must be a whole number");
      const tone = optStr(input, "tone");
      const params: SandboxStartParams = { minutes, ...(typeof seed === "number" ? { seed } : {}), ...(tone === undefined ? {} : { tone }) };
      const r = await ctx.write("sandbox.start", params);
      return { session_id: r.session_id, stage_url: r.stage_url, next: "query{method: sandbox.status} to watch; list_sessions once it is done" };
    },
  },
  {
    name: "stop_sandbox",
    title: "Stop the sandbox session",
    description: "Stops the running sandbox session (the latest one sandbox.status reports): the closing window starts at once and the host signs off. Refuses when no session is running. Returns its status.",
    cls: "GATED",
    input: obj({}),
    run: async (_input, ctx) => {
      const now = await ctx.read("sandbox.status", {});
      if (now.state !== "running") fail("refused", `no sandbox session is running (the latest, ${now.session_id}, is ${now.state})`);
      const r = await ctx.write("sandbox.stop", { session_id: now.session_id });
      return { session_id: r.session_id, state: r.state, elapsed_s: r.elapsed_s };
    },
  },
];
