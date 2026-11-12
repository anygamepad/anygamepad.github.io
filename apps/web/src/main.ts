import "./style.css";

import init, { version } from "./wasm/anygamepad_wasm.js";
import App from "./App.html";
import { emptyView, tabOf, Tester, type TesterView } from "./tester";
import { applyTheme, storedTheme, type Theme } from "./theme";

// Before the WebAssembly loads, so the page does not show the other theme
// first.
const theme = storedTheme();
applyTheme(theme, false);

await init();

// The tester at `/`, the activity log at `/#activity`, the test mode at
// `/test/` (test/index.html): tabs of one page, switched in place.
const mode = tabOf(window.location, document.body.dataset.page);

const tester = new Tester();
App.mount("#app", {
  version: version(),
  theme,
  setTheme: (next: Theme) => {
    applyTheme(next, true);
    (App.data as unknown as { theme: Theme }).theme = next;
  },
  ...emptyView(),
  mode,
  select: tester.select,
  toggleValues: tester.toggleValues,
  startWizard: tester.startWizard,
  connectHid: tester.connectHid,
  downloadLog: tester.downloadLog,
  player: tester.player,
  setVibration: tester.setVibration,
  setPattern: tester.setPattern,
  setPadButtons: tester.setPadButtons,
  setPadButton: tester.setPadButton,
  playVibration: tester.playVibration,
  stopVibration: tester.stopVibration,
  setLight: tester.setLight,
  readInfo: tester.readInfo,
  setTab: tester.setTab,
  setActivityInputs: tester.setActivityInputs,
  saveCurrentReport: tester.saveCurrentReport,
  downloadSavedReport: tester.downloadSavedReport,
  deleteSavedReport: tester.deleteSavedReport,
  openShare: tester.openShare,
  closeShare: tester.closeShare,
  copyAndOpenIssue: tester.copyAndOpenIssue,
  ...tester.settingsActions,
  ...tester.testActions,
});
tester.start(App.data as unknown as TesterView);
