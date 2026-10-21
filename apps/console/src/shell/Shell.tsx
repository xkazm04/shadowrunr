// The shell: the wall, the poster in front (layer 1), the spine and spread (layer 2), the mixer
// bus, the chrome and the intro. It owns no page content: pages plug in through `PageRegistry`.
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { emitKeyCommand } from "../kit/keyCommands";
import { KEYMAP, installKeymap } from "../keys";
import { StatePanel } from "../ui/States";
import { WALL, crumbs, encodeRoute, layerOf, pageRoute, up, useRoute, useRouter, type PageId } from "../router";
import { matchSub } from "../router/schema";
import { Chrome } from "./Chrome";
import { INTRO, Intro } from "./Intro";
import { effectOf, modeOf, type KeyContext } from "./keyActions";
import { BUS_REM, MixerBus } from "./MixerBus";
import { pageLabeller, riseKey } from "./rise";
import styles from "./Shell.module.css";
import { Spread } from "./Spread";
import type { PageRegistry } from "./types";
import { useViewport } from "./useViewport";
import { Wall } from "./Wall";

export interface ShellProps {
  readonly pages: PageRegistry;
  /** Play the intro (fresh boot on the default route, motion allowed). */
  readonly playIntro: boolean;
}

const ON_AIR: PageId = "onair";

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
  const ctx = useRef<KeyContext>({ route, intro, lastPage, hover });
  useLayoutEffect(() => {
    ctx.current = { route, intro, lastPage, hover };
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
          else if (fx.kind === "command") emitKeyCommand(fx.command);
        },
      }),
    [router, finishIntro],
  );

  const registerHit = useCallback((i: number, el: HTMLButtonElement | null) => {
    hitRefs.current[i] = el;
  }, []);

  const labelOf = useMemo(() => pageLabeller(props.pages), [props.pages]);
  const trail = useMemo(() => crumbs(route, labelOf), [route, labelOf]);
  const toggleWall = useCallback(
    () => router.navigate(route.view === "wall" ? pageRoute(lastPage) : WALL),
    [router, route.view, lastPage],
  );
  const goUp = useCallback(() => {
    const parent = up(route);
    if (parent) router.navigate(parent);
  }, [router, route]);

  // A sub the page's declared schema rejects is canonicalised like an unknown page id: replaced
  // (never pushed) with the deepest prefix the schema accepts, and the page is not mounted on it.
  const routes = route.view === "page" ? props.pages[route.page].routes : undefined;
  const subMatch = routes && route.view === "page" ? matchSub(routes, route.sub) : undefined;
  const fallback = subMatch && !subMatch.ok && route.view === "page" ? pageRoute(route.page, ...subMatch.fallback) : null;
  useLayoutEffect(() => {
    if (fallback) router.navigate(fallback, { replace: true });
  }, [router, fallback]);

  const busRem = layer === "l2" || viewport.narrow ? BUS_REM.compact : BUS_REM.full;
  const showBus = !intro && (layer === "l1" || layer === "l2");
  const Layer2 = route.view === "page" ? props.pages[route.page].Layer2 : null;

  return (
    <div className={styles.app} data-layer={layer} data-narrow={viewport.narrow} data-role="shell">
      <Wall
        pages={props.pages}
        view={layer}
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
        {Layer2 && route.view === "page" && route.sub.length > 0 && !fallback && (
          <Suspense fallback={<StatePanel state="loading" title="Loading" />}>
            <Layer2 key={riseKey(route.page, route.sub, routes)} sub={route.sub} />
          </Suspense>
        )}
      </Spread>
      {showBus && <MixerBus pages={props.pages} onAir={active} compact={layer === "l2"} narrow={viewport.narrow} />}
      {!intro && <Chrome onWall={route.view === "wall"} narrow={viewport.narrow} onToggleWall={toggleWall} />}
      {intro && <Intro narrow={viewport.narrow} onSkip={finishIntro} />}
    </div>
  );
}

