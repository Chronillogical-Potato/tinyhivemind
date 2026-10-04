import { defineConfig } from "vite";
import { runsPlugin } from "./server/plugin.js";

// The viewer is a dev tool: `npm run dev` serves the page and an /api that
// reads run traces from disk. `vite build` still emits a static page, which
// then only supports drag/drop.
export default defineConfig({
  plugins: [runsPlugin()],
  server: { allowedHosts: true },
});
