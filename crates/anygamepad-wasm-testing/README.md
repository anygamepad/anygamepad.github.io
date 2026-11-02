# anygamepad-wasm-testing

WebAssembly (wasm-bindgen) bindings of anygamepad's test mode for the web
app: the controller catalog, the lab inventory, the test plans, automatic
detection and the test runner (roadmap tasks H4 and W9,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md)).

It is a module of its own, loaded by the web app's test mode page
(`/test/`) only, so the tester does not download it. It reads no device: the
tester's module (`anygamepad-wasm`) gives it each device
(`Gamepads.testDevice()`, `HidDevices.testDevice()`) and, once a frame,
what the device read (`testInput()`), and sends the output commands the
session asks for. The two modules share no memory, only these values.

Part of [anygamepad](https://github.com/anygamepad/anygamepad.github.io).
Not usable yet: the project is in its first development phase.
