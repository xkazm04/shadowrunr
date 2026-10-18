import { Layer1Placeholder } from "../../kit";

/** Placeholder: P2 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="characters" title="The Cast" owner="P2" nested={{ segment: "malakor", label: "Malakor · package" }} />;
}
