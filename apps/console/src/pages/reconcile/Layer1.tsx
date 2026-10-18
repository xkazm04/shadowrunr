import { Layer1Placeholder } from "../../kit";

/** Placeholder: P6 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="reconcile" title="Morning After" owner="P6" nested={{ segment: "plan", label: "Plan options" }} />;
}
