import { DataProvider, type DataLayer } from "../data";
import { RouterProvider, type RouterStore } from "../router";
import { Shell } from "../shell";
import { PAGES } from "./pages";

export function App(props: { router: RouterStore; data: DataLayer; playIntro: boolean }) {
  return (
    <RouterProvider router={props.router}>
      <DataProvider layer={props.data}>
        <Shell pages={PAGES} playIntro={props.playIntro} />
      </DataProvider>
    </RouterProvider>
  );
}
