import { createContext, useContext, useSyncExternalStore, type MouseEvent, type ReactNode } from "react";
import { encodeRoute, type Route } from "./route";
import type { RouterStore } from "./store";

const RouterContext = createContext<RouterStore | null>(null);

export function RouterProvider({ router, children }: { router: RouterStore; children: ReactNode }) {
  return <RouterContext.Provider value={router}>{children}</RouterContext.Provider>;
}

export function useRouter(): RouterStore {
  const r = useContext(RouterContext);
  if (r === null) throw new Error("useRouter outside <RouterProvider>");
  return r;
}

export function useRoute(): Route {
  const r = useRouter();
  return useSyncExternalStore(r.subscribe, r.getRoute, r.getRoute);
}

function isPlainClick(e: MouseEvent): boolean {
  return e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;
}

/**
 * A link to a route. A plain click navigates in place and records the element's rect, so the
 * shell can push the nested page in from where the operator clicked. Modified clicks keep the
 * browser's behaviour (a new tab, for example).
 */
export function RouteLink(props: {
  to: Route;
  children: ReactNode;
  className?: string | undefined;
  title?: string;
  "aria-label"?: string;
  "aria-current"?: "page" | "true" | undefined;
  "data-role"?: string;
  "data-page"?: string;
}) {
  const router = useRouter();
  const { to, children, ...rest } = props;
  return (
    <a
      {...rest}
      href={encodeRoute(to)}
      onClick={(e) => {
        if (!isPlainClick(e)) return;
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        router.navigate(to, { origin: { left: r.left, top: r.top, width: r.width, height: r.height } });
      }}
    >
      {children}
    </a>
  );
}
