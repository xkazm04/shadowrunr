import { Layer1Placeholder } from "../../kit";

/** Placeholder: P1 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="onair" title="On Air" owner="P1" nested={{ segment: "rundown", label: "The rundown" }} />;
}
