import { Layer2Placeholder } from "../../kit";

/** Placeholder: P1 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="On Air" owner="P1" sub={sub} />;
}
