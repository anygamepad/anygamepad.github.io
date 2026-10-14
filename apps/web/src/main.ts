import "./style.css";

import init, { version } from "./wasm/anygamepad_wasm.js";
import App from "./App.html";
import { emptyView, Tester, type TesterView } from "./tester";
import { applyTheme, storedTheme, type Theme } from "./theme";

// Before the WebAssembly loads, so the page does not show the other theme
// first.
const theme = storedTheme();
applyTheme(theme, false);

await init();

const tester = new Tester();
App.mount("#app", {
  version: version(),
  theme,
  setTheme: (next: Theme) => {
    applyTheme(next, true);
    (App.data as unknown as { theme: Theme }).theme = next;
  },
  ...emptyView(),
  select: tester.select,
  toggleValues: tester.toggleValues,
  connectHid: tester.connectHid,
  downloadLog: tester.downloadLog,
  rumble: tester.rumble,
});
tester.start(App.data as unknown as TesterView);
