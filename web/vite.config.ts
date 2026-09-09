import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  /*
   * A stamp the page can show, so "has my phone actually loaded the new code?"
   * has an answer that is not a guess. It is the moment this config was
   * evaluated: the build time for `npm run build`, and the server start time in
   * development - which is exactly the question, because a phone holding a page
   * from before the last restart is the usual reason a change appears not to
   * have landed.
   */
  /* Tailwind generates the stylesheet from the class names it finds in the
     pages and in the TypeScript that builds markup; `src/theme.css` says where
     to look. Nothing else in `web/` is a stylesheet any more. */
  plugins: [tailwindcss()],
  define: {
    __BUILD__: JSON.stringify(
      new Date().toLocaleString("en-GB", { hour12: false }).replace(",", ""),
    ),
  },
  // The phone loads this from another device, so bind to every interface.
  server: { host: true, port: 5173, strictPort: true },
  build: {
    target: "es2022",
    rollupOptions: {
      // Five pages, all sharing `src/theme.css`: the phone surface, the
      // settings page that replaced hand-editing the config file, the debug
      // view for the computer, a standalone haptics probe for a device that
      // will not buzz, and a bench for the press animation - the one thing
      // that cannot be reviewed from code or from a screenshot.
      //
      // The connect screen is not among them: it is served by the app itself,
      // because only the app can draw the QR and hand the page the secret.
      input: {
        main: "index.html",
        config: "config.html",
        debug: "debug.html",
        haptics: "haptics.html",
        preview: "preview.html",
      },
    },
  },
});
