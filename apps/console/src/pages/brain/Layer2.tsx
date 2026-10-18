import { Layer2Placeholder } from "../../kit";

/** Placeholder: P3 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="The Brain" owner="P3" sub={sub} />;
}
