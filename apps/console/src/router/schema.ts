// A page's URL grammar below its page id, declared once as a segment tree. The page parses its
// layer-2 `sub` with `matchSub`, the shell derives breadcrumb labels (`schemaLabel`), the rise
// key (`riseDepth`) and the unknown-sub fallback from the same tree, and the page exports typed
// builders that only produce routes the tree accepts. `route.ts` still owns encoding: a segment
// here is always a decoded string.

interface NodeBase {
  /** What the segment is called in breadcrumbs; `undefined` leaves it to `humanize`. */
  readonly label?: string | ((value: string) => string);
  /**
   * The segment re-renders the page in place (a scroll target, a filter): it does not count
   * toward the shell's rise key, so changing it keeps the page's state.
   */
  readonly inPlace?: true;
  readonly children?: RouteSchema;
}

/** A fixed segment, e.g. `transcripts`. */
export interface LiteralNode extends NodeBase {
  readonly seg: string;
  readonly param?: never;
  readonly test?: never;
}

/** A named variable segment, e.g. a run id; `test` narrows what it accepts. */
export interface ParamNode extends NodeBase {
  readonly param: string;
  readonly seg?: never;
  readonly test?: (value: string) => boolean;
}

export type RouteNode = LiteralNode | ParamNode;

/** The segments accepted right below the page id (or below a node). An empty sub always matches. */
export type RouteSchema = readonly RouteNode[];

export type SubMatch =
  | {
      readonly ok: true;
      /** Every param segment on the matched path, by name. */
      readonly params: Readonly<Record<string, string>>;
      /** How many leading segments key the page's layer 2 (the shell's rise). */
      readonly riseDepth: number;
    }
  /** The sub named nothing the schema declares; `fallback` is its deepest accepted prefix. */
  | { readonly ok: false; readonly fallback: readonly string[] };

/** Literals first, then params, so `new` beats `:id` at the same depth. */
function pick(schema: RouteSchema, segment: string): RouteNode | undefined {
  return schema.find((n) => n.seg === segment) ?? schema.find((n) => n.param !== undefined && (n.test?.(segment) ?? true));
}

/** The nodes a sub walks through, as far as the schema accepts it. */
function walk(schema: RouteSchema, sub: readonly string[]): RouteNode[] {
  const path: RouteNode[] = [];
  let level: RouteSchema = schema;
  for (const segment of sub) {
    const node = pick(level, segment);
    if (!node) break;
    path.push(node);
    level = node.children ?? [];
  }
  return path;
}

export function matchSub(schema: RouteSchema, sub: readonly string[]): SubMatch {
  const path = walk(schema, sub);
  if (path.length < sub.length) return { ok: false, fallback: sub.slice(0, path.length) };
  const params: Record<string, string> = {};
  let riseDepth = 0;
  path.forEach((node, i) => {
    if (node.param !== undefined) params[node.param] = sub[i] ?? "";
    if (!node.inPlace) riseDepth = i + 1;
  });
  return { ok: true, params, riseDepth };
}

/**
 * Breadcrumb labels for a page from its schema, in the router's `LabelOf` shape: `parts` is the
 * whole route (page id first), so depth 1 is the first sub segment.
 */
export function schemaLabel(schema: RouteSchema): (segment: string, depth: number, parts: readonly string[]) => string | undefined {
  return (segment, depth, parts) => {
    if (depth < 1) return undefined;
    const path = walk(schema, parts.slice(1, depth + 1));
    if (path.length !== depth) return undefined;
    const label = path[depth - 1]?.label;
    return typeof label === "function" ? label(segment) : label;
  };
}
