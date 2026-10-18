// A TOML reader for the subset the contest's package files use (fidelity harness only, never
// bundled): comments, [table], [a.b], [[array]], basic and multi-line basic strings (with line
// continuation), numbers, booleans, arrays (multi-line, nested) and inline tables.

export type TomlValue = string | number | boolean | TomlValue[] | TomlTable;
export interface TomlTable {
  [key: string]: TomlValue;
}

const isTable = (v: TomlValue | undefined): v is TomlTable => typeof v === "object" && !Array.isArray(v);

class Reader {
  private i = 0;
  private readonly s: string;
  constructor(s: string) {
    this.s = s;
  }

  done(): boolean {
    return this.i >= this.s.length;
  }

  private peek(n = 0): string {
    return this.s[this.i + n] ?? "";
  }

  /** Skips spaces, tabs, and (when `lines`) newlines and comments. */
  private gap(lines: boolean): void {
    for (;;) {
      const c = this.peek();
      if (c === " " || c === "\t" || c === "\r" || (lines && c === "\n")) this.i++;
      else if (c === "#" && lines) while (!this.done() && this.peek() !== "\n") this.i++;
      else return;
    }
  }

  private fail(what: string): never {
    throw new Error(`toml: ${what} at offset ${this.i}`);
  }

  private key(): string {
    this.gap(false);
    if (this.peek() === '"') return this.basic();
    const m = /^[A-Za-z0-9_-]+/.exec(this.s.slice(this.i));
    if (!m) this.fail("expected a key");
    this.i += m[0].length;
    return m[0];
  }

  private dotted(): string[] {
    const parts = [this.key()];
    this.gap(false);
    while (this.peek() === ".") {
      this.i++;
      parts.push(this.key());
      this.gap(false);
    }
    return parts;
  }

  private basic(): string {
    if (this.s.startsWith('"""', this.i)) return this.multi();
    this.i++;
    let out = "";
    while (this.peek() !== '"') {
      if (this.done() || this.peek() === "\n") this.fail("unterminated string");
      out += this.peek() === "\\" ? this.escape() : this.s[this.i++];
    }
    this.i++;
    return out;
  }

  private multi(): string {
    this.i += 3;
    if (this.peek() === "\r") this.i++;
    if (this.peek() === "\n") this.i++;
    let out = "";
    while (!this.s.startsWith('"""', this.i)) {
      if (this.done()) this.fail("unterminated multi-line string");
      if (this.peek() === "\\" && /^\\[ \t]*\r?\n/.test(this.s.slice(this.i))) {
        this.i++;
        while (/[ \t\r\n]/.test(this.peek())) this.i++;
      } else out += this.peek() === "\\" ? this.escape() : this.s[this.i++];
    }
    this.i += 3;
    return out;
  }

  private escape(): string {
    const c = this.peek(1);
    this.i += 2;
    const simple: Record<string, string> = { n: "\n", t: "\t", r: "\r", '"': '"', "\\": "\\" };
    const hit = simple[c];
    if (hit !== undefined) return hit;
    if (c === "u" || c === "U") {
      const len = c === "u" ? 4 : 8;
      const hex = this.s.slice(this.i, this.i + len);
      this.i += len;
      return String.fromCodePoint(Number.parseInt(hex, 16));
    }
    return this.fail(`bad escape \\${c}`);
  }

  value(): TomlValue {
    this.gap(false);
    const c = this.peek();
    if (c === '"') return this.basic();
    if (c === "[") return this.array();
    if (c === "{") return this.inline();
    const m = /^(true|false|[+-]?[0-9][0-9_]*(\.[0-9_]+)?([eE][+-]?[0-9]+)?)/.exec(this.s.slice(this.i));
    if (!m) this.fail("expected a value");
    this.i += m[0].length;
    if (m[0] === "true" || m[0] === "false") return m[0] === "true";
    return Number(m[0].replace(/_/g, ""));
  }

  private array(): TomlValue[] {
    this.i++;
    const out: TomlValue[] = [];
    for (;;) {
      this.gap(true);
      if (this.peek() === "]") break;
      out.push(this.value());
      this.gap(true);
      if (this.peek() === ",") this.i++;
    }
    this.i++;
    return out;
  }

  private inline(): TomlTable {
    this.i++;
    const out: TomlTable = {};
    for (;;) {
      this.gap(false);
      if (this.peek() === "}") break;
      const path = this.dotted();
      if (this.peek() !== "=") this.fail("expected =");
      this.i++;
      assign(out, path, this.value());
      this.gap(false);
      if (this.peek() === ",") this.i++;
    }
    this.i++;
    return out;
  }

  /** One statement: a table header or a key/value pair, onto `root`; returns the new cursor table. */
  statement(root: TomlTable, cur: TomlTable): TomlTable | null {
    this.gap(true);
    if (this.done()) return null;
    if (this.peek() === "[") {
      const arr = this.peek(1) === "[";
      this.i += arr ? 2 : 1;
      const path = this.dotted();
      this.i += arr ? 2 : 1;
      return arr ? appendTable(root, path) : openTable(root, path);
    }
    const path = this.dotted();
    if (this.peek() !== "=") this.fail("expected =");
    this.i++;
    assign(cur, path, this.value());
    return cur;
  }
}

function descend(t: TomlTable, key: string): TomlTable {
  const v = t[key];
  if (Array.isArray(v)) {
    const last = v[v.length - 1];
    if (isTable(last)) return last;
  }
  if (isTable(v)) return v;
  const fresh: TomlTable = {};
  t[key] = fresh;
  return fresh;
}

function assign(t: TomlTable, path: readonly string[], v: TomlValue): void {
  let at = t;
  for (const k of path.slice(0, -1)) at = descend(at, k);
  const last = path[path.length - 1];
  if (last !== undefined) at[last] = v;
}

function openTable(root: TomlTable, path: readonly string[]): TomlTable {
  let at = root;
  for (const k of path) at = descend(at, k);
  return at;
}

function appendTable(root: TomlTable, path: readonly string[]): TomlTable {
  let at = root;
  for (const k of path.slice(0, -1)) at = descend(at, k);
  const last = path[path.length - 1] ?? "";
  const existing = at[last];
  const list: TomlValue[] = Array.isArray(existing) ? existing : [];
  const fresh: TomlTable = {};
  list.push(fresh);
  at[last] = list;
  return fresh;
}

export function parseToml(src: string): TomlTable {
  const root: TomlTable = {};
  const r = new Reader(src);
  let cur: TomlTable | null = root;
  while (cur !== null) cur = r.statement(root, cur);
  return root;
}
