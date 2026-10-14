import "./style.css";

import init, { version } from "./wasm/anygamepad_wasm.js";
import App from "./App.html";
import { emptyView, Tester, type TesterView } from "./tester";

await init();

const tester = new Tester();
App.mount("#app", {
  version: version(),
  ...emptyView(),
  select: tester.select,
  toggleValues: tester.toggleValues,
  connectHid: tester.connectHid,
  rumble: tester.rumble,
});
tester.start(App.data as unknown as TesterView);
