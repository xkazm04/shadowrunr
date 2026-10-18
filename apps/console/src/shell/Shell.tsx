// The shell: the wall, the poster in front (layer 1), the spine and spread (layer 2), the mixer
// bus, the chrome and the intro. It owns no page content: pages plug in through `PageRegistry`.
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { KEYMAP, installKeymap } from "../keys";
import { StatePanel } from "../ui/States";
import { PAGE_IDS, WALL, crumbs, encodeRoute, layerOf, pageRoute, up, useRoute, useRouter, type LabelOf, type PageId } from "../router";
import { Chrome } from "./Chrome";
import { INTRO, Intro } from "./Intro";
import { effectOf, modeOf, type KeyContext } from "./keyActions";
import { BUS_REM, MixerBus } from "./MixerBus";
import { POSTERS } from "./posters";
import styles from "./Shell.module.css";
import { Spread } from "./Spread";
import type { PageRegistry } from "./types";
import { useViewport } from "./useViewport";
import { Wall } from "./Wall";

export interface ShellProps {
  readonly pages: PageRegistry;
  /** Play the intro (fresh boot on the default route, motion allowed). */
  readonly playIntro: boolean;
  /** The legacy desk's view ids, for its Ctrl+1..5 bindings. */
  readonly legacyViews: readonly string[];
  readonly renderLegacy: (sub: readonly string[], navigate: (view: string) => void) => ReactNode;
}

const ON_AIR: PageId = "onair";

function labelFor(pages: PageRegistry): LabelOf {
  return (segment, depth, parts) => {
    const page = PAGE_IDS.find((id) => id === parts[0]);
    if (depth === 0) return page ? POSTERS[page].name : parts[0] === "legacy" ? "Legacy desk" : undefined;
    return page ? pages[page].label?.(segment, depth, parts) : undefined;
  };
}

export function Shell(props: ShellProps) {
  const router = useRouter();
  const route = useRoute();
  const viewport = useViewport();
  const [intro, setIntro] = useState(props.playIntro);
  const [hover, setHover] = useState<number | null>(null);
  const [still, setStill] = useState(!props.playIntro);
  const [lastPage, setLastPage] = useState<PageId>(route.view === "page" ? route.page : ON_AIR);
  const hitRefs = useRef<(HTMLButtonElement | null)[]>([]);
  // Adjusting state from the route while rendering (React's documented pattern, no ref write).
  if (route.view === "page" && route.page !== lastPage) setLastPage(route.page);

  const layer = layerOf(route);
  const view = layer === "legacy" ? "wall" : layer;
  const active: PageId = route.view === "page" ? route.page : lastPage;

  // `still` suppresses the poster transitions on the very first paint after a deep link.
  useEffect(() => {
    if (!still) return;
    const f = requestAnimationFrame(() => requestAnimationFrame(() => setStill(false)));
    return () => cancelAnimationFrame(f);
  }, [still]);

  const finishIntro = useCallback(() => {
    setIntro(false);
    setHover(null);
    router.navigate(pageRoute(ON_AIR), { replace: true });
  }, [router]);

  useEffect(() => {
    if (!intro) return;
    const hoverAt = window.setTimeout(() => setHover(0), INTRO.hoverAtMs);
    const end = window.setTimeout(finishIntro, INTRO.endMs);
    return () => {
      window.clearTimeout(hoverAt);
      window.clearTimeout(end);
    };
  }, [intro, finishIntro]);

  // The one keymap listener; the context is read through a ref so it is installed once.
  const ctx = useRef<KeyContext>({ route, intro, lastPage, hover, legacyViews: props.legacyViews });
  useLayoutEffect(() => {
    ctx.current = { route, intro, lastPage, hover, legacyViews: props.legacyViews };
  });
  useEffect(
    () =>
      installKeymap(window, {
        keymap: KEYMAP,
        getMode: () => modeOf(ctx.current),
        dispatch: (action) => {
          const fx = effectOf(action, ctx.current);
          if (fx.kind === "navigate") router.navigate(fx.route);
          else if (fx.kind === "hover") {
            setHover(fx.index);
            hitRefs.current[fx.index]?.focus({ preventScroll: true });
          } else if (fx.kind === "skip-intro") finishIntro();
          else if (fx.kind === "blur" && document.activeElement instanceof HTMLElement) document.activeElement.blur();
        },
      }),
    [router, finishIntro],
  );

  const registerHit = useCallback((i: number, el: HTMLButtonElement | null) => {
    hitRefs.current[i] = el;
  }, []);

  const labelOf = useMemo(() => labelFor(props.pages), [props.pages]);
  const trail = useMemo(() => crumbs(route, labelOf), [route, labelOf]);
  const toggleWall = useCallback(
    () => router.navigate(route.view === "wall" ? pageRoute(lastPage) : WALL),
    [router, route.view, lastPage],
  );
  const goUp = useCallback(() => {
    const parent = up(route);
    if (parent) router.navigate(parent);
  }, [router, route]);

  const busRem = view === "l2" || viewport.narrow ? BUS_REM.compact : BUS_REM.full;
  const showBus = !intro && (layer === "l1" || layer === "l2");
  const Layer2 = route.view === "page" ? props.pages[route.page].Layer2 : null;

  return (
    <div className={styles.app} data-layer={layer} data-narrow={viewport.narrow} data-role="shell">
      {layer === "legacy" && route.view === "legacy" ? (
        <div className={styles.legacy} data-role="legacy">
          {props.renderLegacy(route.sub, (v) => router.navigate({ view: "legacy", sub: [v] }))}
        </div>
      ) : (
        <>
          <Wall
            pages={props.pages}
            view={view}
            active={active}
            hover={hover}
            onHover={setHover}
            viewport={viewport}
            busRem={busRem}
            pre={intro}
            still={still}
            crumbs={trail}
            onOpen={(id) => router.navigate(pageRoute(id))}
            onBack={goUp}
            registerHit={registerHit}
          />
          <Spread
            open={layer === "l2"}
            pageKey={encodeRoute(route)}
            takeOrigin={router.takeOrigin}
            narrow={viewport.narrow}
            busOffset={showBus ? `${busRem}rem` : "0rem"}
            label={trail.map((c) => c.label).join(" / ")}
          >
            {Layer2 && route.view === "page" && route.sub.length > 0 && (
              <Suspense fallback={<StatePanel state="loading" title="Loading" />}>
                <Layer2 sub={route.sub} />
              </Suspense>
            )}
          </Spread>
          {showBus && <MixerBus pages={props.pages} onAir={active} compact={layer === "l2"} narrow={viewport.narrow} />}
        </>
      )}
      {!intro && <Chrome onWall={route.view === "wall"} narrow={viewport.narrow} onToggleWall={toggleWall} />}
      {intro && <Intro narrow={viewport.narrow} onSkip={finishIntro} />}
    </div>
  );
}

