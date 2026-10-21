// Vitest setup (vite.config.ts `test.setupFiles`): the contract loads each method's result
// validator as its own module on that method's first reply. Page tests count render rounds
// (`settle(n)`), and a cold module load is not a round, so every test file starts with the
// validators already loaded into its own module graph.
import { preloadResultValidators } from "@shadowrunr/contract";

await preloadResultValidators();
