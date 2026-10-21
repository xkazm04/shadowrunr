// The figure's rest matte, made whole. The union of the rig's solids leaves gaps the portrait
// filled with its dark ground (between the cheekbones, the jaw and the collar, the neck behind the
// jaw): the glow drew the backplate there, a cut-out must not show the page through them. So the
// union is closed (dilated then eroded, which seals narrow channels to the outside) and every hole
// that no longer reaches the border is filled. Pure, on one byte a pixel, so the tests read it.

/** Grow (max) or shrink (min) by a square of radius `r`, separably. */
function morph(src: Uint8Array, n: number, r: number, grow: boolean): Uint8Array {
  const pick = grow ? Math.max : Math.min;
  const tmp = new Uint8Array(n * n);
  const out = new Uint8Array(n * n);
  for (let y = 0; y < n; y++)
    for (let x = 0; x < n; x++) {
      let v = src[y * n + x] ?? 0;
      for (let k = -r; k <= r; k++) {
        const xx = x + k;
        if (xx >= 0 && xx < n) v = pick(v, src[y * n + xx] ?? 0);
        else if (!grow) v = 0; // past the border is outside the figure
      }
      tmp[y * n + x] = v;
    }
  for (let y = 0; y < n; y++)
    for (let x = 0; x < n; x++) {
      let v = tmp[y * n + x] ?? 0;
      for (let k = -r; k <= r; k++) {
        const yy = y + k;
        if (yy >= 0 && yy < n) v = pick(v, tmp[yy * n + x] ?? 0);
        else if (!grow) v = 0;
      }
      out[y * n + x] = v;
    }
  return out;
}

/** Close `src` (n x n) by `r`: gaps narrower than about 2r are sealed, the outline is kept. */
export function closeMatte(src: Uint8Array, n: number, r: number): Uint8Array {
  return r > 0 ? morph(morph(src, n, r, true), n, r, false) : src.slice();
}

/**
 * Fill every hole of `src` (n x n): a pixel under `threshold` that cannot reach the border through
 * other pixels under it becomes fully covered. The rest is kept as it was.
 */
export function fillHoles(src: Uint8Array, n: number, threshold = 128): Uint8Array {
  const out = src.slice();
  const seen = new Uint8Array(n * n);
  const stack: number[] = [];
  const push = (i: number) => {
    if (!seen[i] && (src[i] ?? 0) < threshold) {
      seen[i] = 1;
      stack.push(i);
    }
  };
  for (let i = 0; i < n; i++) {
    push(i);
    push((n - 1) * n + i);
    push(i * n);
    push(i * n + n - 1);
  }
  while (stack.length) {
    const i = stack.pop() as number;
    const x = i % n;
    const y = (i - x) / n;
    if (x > 0) push(i - 1);
    if (x < n - 1) push(i + 1);
    if (y > 0) push(i - n);
    if (y < n - 1) push(i + n);
  }
  for (let i = 0; i < n * n; i++) if (!seen[i] && (src[i] ?? 0) < threshold) out[i] = 255;
  return out;
}

/** The whole matte: the union closed by `r`, its holes filled, never less than the union itself. */
export function wholeMatte(union: Uint8Array, n: number, r: number): Uint8Array {
  const filled = fillHoles(closeMatte(union, n, r), n);
  for (let i = 0; i < filled.length; i++) filled[i] = Math.max(filled[i] ?? 0, union[i] ?? 0);
  return filled;
}
