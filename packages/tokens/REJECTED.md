# Rejected token candidates

A repeated literal from the winner (The Poster Wall) becomes a role only when it passes all three
admission tests:

1. it has a recurring intent;
2. it has a one-sentence definition (each role's `description` in `tokens.source.json`, which a
   unit test enforces);
3. it is not a name-tagged raw value.

Fidelity roles cannot collapse: the layer-1 graphics, the print texture and the per-page palettes.
A literal used only on one of them is kept exactly. It is either admitted under a scoped group
(`screen-*`, `halftone-*`) or left in its component as a `rem` literal. Layer-2, chrome and spine
values are "kept in spirit", so they may collapse onto an admitted role. Every collapse below says
what changes, from the old value to the new one.

Sources read: `poster.css`, `l1.css`, `l2.css`, `app.js` (`PAGES`, `SA`) and `sr-kit.js`
(`RANK_COLORS`, `GRADE_COLORS`).

## Colours

| Candidate | Uses | Verdict and reason |
|---|---|---|
| `--bone: #efe6d2` | declared in `:root` | **Rejected, duplicate.** It is a second name for `--paper`, a name-tagged raw value with no separate intent. It is merged into `color.paper`. |
| `--paper`, `--ember`, `--teal` and the rest of the `:root` names | 14 | **Kept as primitives, not as roles.** Colour names tag a raw value. The roles name intents instead, such as `surface-paper`, `signal`, `ok-bright` and `fail`. |
| `PAGES[].acc` | 6 | **Rejected, dead field.** `app.js` never reads it, and it equals `SA[1]` on every page. It is merged into the page role `accent-text`. |
| `#f6efdc` | 19 | **Admitted** as `surface-card`. |
| `#1b6150` | 10 (5 CSS, 5 JS) | **Admitted** as `ok`, the held ink on paper, and as `accent-text` for Characters and Brain. |
| `#14110d` and `rgba(20,17,13,a)` | 5 + 7 | **Admitted** as `surface-press`, with the alpha roles `line`, `wash` and `track`. |
| `#000` in mask gradients | 6 | **Admitted** as `mask-ink`: the opaque stop of an alpha mask. |
| `rgba(0,0,0,.0)` | 2 | **Rejected.** This is the keyword `transparent`, not a colour token. |
| `#d9ceb3` (`.sw i`, the switch track) | 1, layer 2 | **Collapsed** into `surface-chip`: `#d9ceb3` → `#e3d6b8`. |
| `#f6dcc8` (`.gh.nx`, the next-to-test ghost) | 1, layer 2 | **Collapsed** into `surface-selected`: `#f6dcc8` → `#fbe6d6`. |
| `#2d2820` (`.lede`) | 1, layer 2 | **Collapsed** into `text-note`: `#2d2820` → `#3a342a`. |
| `#26211a` (`.voice`) | 1, layer 2 | **Collapsed** into `text-ink`: `#26211a` → `#161411`. |
| `#b9ac8e` (`.tone-hero .kv dt`) | 1, layer 2 | **Collapsed** into `text-mute-on-dark`: `#b9ac8e` → `#cdbf9f`. |
| `rgba(20,17,13,.12)` (`.rbar`) | 1, layer 2 | **Collapsed** into `track`: alpha .12 → .14. |
| `rgba(232,84,42,.1)` (`.tbl tbody tr:hover`) | 1, layer 2 | **Collapsed** into `row-hover`: alpha .1 → .12. |
| `rgba(232,84,42,.3)` (`.tl-list .vr.sel`) | 1, layer 2 | **Collapsed** into `row-selected`: alpha .3 → .2. |
| Single alpha steps on layer-1 graphics: paper .07/.16/.2/.3/.35/.45/.5/.6/.8, ink .25/.7/.82, scrim .8/.88/.96, black .2/.45, teal .9, blood .9, ochre .7/.16 | 1 to 2 each | **Not roles.** Each one is a one-off on a fidelity graphic, so the value is kept exactly. The component writes `color-mix(in srgb, var(--sr-color-<role>) <a×100>%, transparent)` over an admitted colour role, which is exactly `rgba(<role>, <a>)` (WPI settled this: stylelint bans `rgb()`, so the R G B channel roles were retired and `scrim` became the colour role `--sr-color-scrim`). |
| `#0d1312`, `#16231f`, `#1c2e29`, `#1a2421`, `#e5dcc7` | 1 each, the On Air screen | **Admitted under `screen-*`.** Each is single-use in CSS, but together they are the broadcast screen's set palette. That palette recurs across On Air layer 1, the mixer-bus mini-renders and the stage. The values are kept exactly. |
| Status chip colours `#bfe0d3`/`#0f3a2f`, `#f0b9a5`/`#5a1708`, `#e0d6ff`/`#5a4a9a`/`#2a2060` | 1 each | **Admitted as the `verdict-*` family**, because the verdict chip is one component used on every page. |
| `#ff9c82` | 1 | **Admitted** as `diff-del`, the partner of `diff-add`. |
| `#fff` | 2 | **Admitted** as `text-on-signal`. |
| `#ffb15a` | 2 (plus `art.js`) | **Admitted** as `link-on-press`. |
| Art palettes in `sr-kit.js`, `art.js`, `l1.js` and `stage.js` (`#e7dcc3`, `#b9ab8b`, `#10110f`, `#fff0c0`, `#d9b48f` and others) | | **Out of F1 scope.** These are the Malakor and foil illustration colours, drawn as SVG attributes. They belong to the art components (stage-kit, P1 and P2), not to page CSS. |

## Type sizes

The winner uses about 60 distinct font sizes. They collapse to a **16-step text scale**
(`.8125` to `2.6rem`, roles `micro` to `heading-lg`) plus a root size.

| Candidate | Verdict and reason |
|---|---|
| Display sizes of 3rem and up, each with one intent: `3.5` (spine title), `4.5`, `4.6`, `7.6`, `8`, `8.6` (`--fs`), `9`, `9.4`, `9.5`, `15`, `15.5`, `20` (layer-1 numerals and titles), `3.2`, `3.6`, `4`, `6.4` (layer-2 headings) | **Rejected, single-use composition values.** They stay as exact `rem` literals in their components, which the stylelint rule allows because it bans px, not rem. |
| `5rem` ×3 (narrow intro, `.br-lat .u`, `.tone-hero h2`), `5.6rem` ×2, `11rem` ×2 | **Rejected.** The values repeat but the intents do not. The uses stay as exact literals. |
| `1.35` (`.cb-name .r`), `1.55`, `1.8` (`.sg-pyr`), `2.1` (`.pf-vt`, `.sg-cmd code`), `2.7` (`.rm-t`), `1.6` (narrow `.pf-vt`) | **Rejected, single uses on layer-1 graphics.** They stay exact as component literals. |
| `.75` (narrow `#scaledflag`, chrome) | **Collapsed** to `micro`: `.75` → `.8125rem`. |
| `1.0625` (`.voice.sm`) | **Collapsed** to `control`: `1.0625` → `1.05rem`. |
| `1.35` (`.l2-sec h3`) | **Collapsed** to `subhead`: `1.35` → `1.4rem`. |
| `1.6` (`.tone h3`), `1.65` (`.opt h3`), `1.8` (`.prose.cols h2`, `.hashcard>h2`) | **Collapsed** to `title`: each → `1.7rem`. |
| `1.9` (narrow `.l2-sec>h2`) | **Collapsed** to `figure`: `1.9` → `2rem`. |
| `9px`, `11px`, `13px`, `14px`, `17px` (SVG `<text>` in the hexmap and the sim grid) | **Rejected.** These are SVG user units, so they belong as presentation attributes on `<text>` in the component. A/1's canvas replaces the sim-grid ones. |
| `.875em` (`#spread code`) | **Rejected.** It is relative to its parent, not a step on the rem scale. |
| `html{font-size:16px}` plus the JS `rescale()` | **Admitted** as `root.size`, which is `clamp(16px,min(1.25vw,2vh),30.4px)`, and 16px at or below `narrow`. This is the winner's `16 × clamp(1, min(w/1280, h/800), 1.9)` in CSS, and it ends the resize listener. |
| Line heights and letter spacings | **Not tokenised in F1.** Typography may be rebuilt in spirit, and neither family was among the Q0 candidates. |

## Spacing

The scale has 14 steps: `.1, .2, .3, .4, .5, .6, .7, .8, 1, 1.2, 1.4, 1.6, 2, 2.4rem`
(`hair` to `gutter`).

| Candidate | Verdict and reason |
|---|---|
| Half-steps `.02`, `.05`, `.15`, `.18`, `.25`, `.26`, `.32`, `.35`, `.45`, `.55`, `.65`, `.9`, `1.1`, `1.3`, `1.5`, `2.6`, `3.2`, `4`, `5rem` | **Rejected.** These are half-steps of the same rhythm, not distinct intents. On layer-1 graphics, the poster faces and the screen they stay exact as component literals. On layer 2 they collapse to the nearest step when ported. |
| `23rem` (`.b-top`) and `24rem` (`.l2-tabs`) `padding-right` | **Rejected, smell.** This space is reserved for the fixed chrome. F3 should lay the chrome out in flow instead. |

## Borders, radii, z-index, easing and breakpoints

| Candidate | Verdict and reason |
|---|---|
| `1px` ×23, `2px` ×69, `3px` ×25 | **Admitted** as `stroke-hair`, `stroke-rule` and `stroke-heavy`. Without them, component CSS could not satisfy the no-raw-px rule. |
| `border-radius: .5rem .5rem 0 0` (the lock shackle) and `50% 50% 0 0/24% 24% 0 0` (the portrait arch) | **Rejected.** Each is a single-use shape on a graphic. |
| `border-radius: 2px` ×2, `50%` ×6, `0` ×1 | **Admitted** as `nick`, `round` and `none`. |
| 15 z-index values | **All mapped** to 23 named layers: the app stack, the poster stack, local layers and the screen overlays. The numbers are unchanged, so stacking is unchanged. |
| `ease-in-out`, `linear` | **Rejected.** They are CSS keywords, not values. |
| Durations (`.85s` and the rest) | **Not tokenised in F1.** Motion is kept in spirit and was not among the Q0 candidates. |
| Breakpoints `900px` (`.bible`, `.rules2`) and `1000px` (`.vgrid`) | **Collapsed** into `split` (`1100px`). Both are layer-2 layouts. Consider a container query instead (`promotion.md` §8). |
| Breakpoint `761px` (`min-width`) | **Rejected as a separate token.** It is the complement of `narrow`, written as `MEDIA.aboveNarrow`. |
| `@custom-media` declarations | **Not emitted.** Unprocessed, Lightning CSS warns about the unknown at-rule. Media queries use the literal queries in `MEDIA` instead. |
