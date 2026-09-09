import "./theme.css";
import "./style.css";
import { launchPad } from "./launch";

const stop = launchPad(() => {
  document.getElementById("launch-status")!.hidden = true;
  document.getElementById("app-frame")!.hidden = false;
  // No socket, touch surface or canvas is started on the temporary landing.
  void import("./main");
});

import.meta.hot?.dispose(stop);
