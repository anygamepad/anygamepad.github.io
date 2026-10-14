// Entry point of the privacy page (privacy/index.html): the tester's styles,
// and the theme the viewer picked there. The page has no theme switch.

import "./style.css";

import { applyTheme, storedTheme } from "./theme";

applyTheme(storedTheme(), false);
