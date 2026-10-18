// Placeholders the page builders replace (P1–P6): a layer-1 status view with one lit link into
// a nested layer-2 page, and that layer-2 page with its connection state.
import { RouteLink, pageRoute, type PageId } from "../router";
import { Head } from "../ui/Section";
import { EmptyState } from "../ui/States";
import styles from "./Placeholder.module.css";
import { ConnectionState } from "./QueryView";

export function Layer1Placeholder(props: { page: PageId; title: string; owner: string; nested: { segment: string; label: string } }) {
  return (
    <div className={styles.l1} data-role="layer1-placeholder">
      <span className={styles.kicker}>Layer 1 · {props.owner}</span>
      <h2 className={styles.title}>{props.title}</h2>
      <p className={styles.note}>The graphic status view lands here.</p>
      <RouteLink to={pageRoute(props.page, props.nested.segment)} className={styles.open} data-role="push-in">
        {props.nested.label}
      </RouteLink>
    </div>
  );
}

export function Layer2Placeholder(props: { title: string; owner: string; sub: readonly string[] }) {
  return (
    <div data-role="layer2-placeholder">
      <Head kicker={`Layer 2 · ${props.owner}`} title={props.title} lede={`Nested page: ${props.sub.join(" / ")}`} />
      <ConnectionState>
        <EmptyState title="This page is a placeholder" hint={`${props.owner} builds it on the data layer and shared primitives.`} />
      </ConnectionState>
    </div>
  );
}
