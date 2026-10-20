/** A rect in viewport coordinates (the shape of `getBoundingClientRect()`). */
export interface PushRect {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** The transform that makes an element start at the clicked element's rect (A/1's push-in). */
export function pushFrom(origin: PushRect, target: PushRect): string {
  const sx = Math.max(0.15, origin.width / Math.max(1, target.width));
  const sy = Math.max(0.15, origin.height / Math.max(1, target.height));
  const tx = origin.left + origin.width / 2 - (target.left + target.width / 2);
  const ty = origin.top + origin.height / 2 - (target.top + target.height / 2);
  return `translate(${tx.toFixed(1)}px, ${ty.toFixed(1)}px) scale(${sx.toFixed(3)}, ${sy.toFixed(3)})`;
}
