import { Layer2Placeholder } from "../../kit";

/** Placeholder: P5 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="The Trials" owner="P5" sub={sub} />;
}
