import "./style.css";

import init, { version } from "./wasm/anygamepad_wasm.js";
import App from "./App.html";
import { emptyView, startTester, type TesterView } from "./tester";

await init();

App.mount("#app", { version: version(), ...emptyView() });
startTester(App.data as unknown as TesterView);
