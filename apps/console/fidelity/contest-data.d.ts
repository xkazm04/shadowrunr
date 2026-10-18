// The contest data module the harness server provides (fidelity/serve.mjs), read from the file
// named by SHADOWRUNR_CONTEST_DATA.
declare module "virtual:shadowrunr-contest-data" {
  import type { ContestData } from "./contest";
  const data: ContestData;
  export default data;
}
