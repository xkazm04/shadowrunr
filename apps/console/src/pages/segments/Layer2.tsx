import { Layer2Placeholder } from "../../kit";

/** Placeholder: P4 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="The Programme" owner="P4" sub={sub} />;
}
