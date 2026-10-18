import { Layer2Placeholder } from "../../kit";

/** Placeholder: P6 replaces it with the nested pages. */
export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  return <Layer2Placeholder title="Morning After" owner="P6" sub={sub} />;
}
