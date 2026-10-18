import { Layer2Placeholder } from "../../kit";

/** Placeholder: P2 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="The Cast" owner="P2" sub={sub} />;
}
