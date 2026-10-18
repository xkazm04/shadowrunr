/** The root font size in px (the token root size is fluid), for converting rem estimates. */
export function remPx(): number {
  if (typeof document === "undefined") return 16;
  const px = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  return Number.isFinite(px) && px > 0 ? px : 16;
}
