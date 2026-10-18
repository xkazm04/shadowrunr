import { Layer1Placeholder } from "../../kit";

/** Placeholder: P4 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="segments" title="The Programme" owner="P4" nested={{ segment: "tables", label: "Segment tables" }} />;
}
