import { Layer1Placeholder } from "../../kit";

/** Placeholder: P5 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="simulator" title="The Trials" owner="P5" nested={{ segment: "tournaments", label: "Tournaments" }} />;
}
