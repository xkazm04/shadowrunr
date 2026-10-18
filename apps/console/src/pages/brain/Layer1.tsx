import { Layer1Placeholder } from "../../kit";

/** Placeholder: P3 replaces it with the layer-1 graphic. */
export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <Layer1Placeholder page="brain" title="The Brain" owner="P3" nested={{ segment: "transcripts", label: "Transcripts" }} />;
}
