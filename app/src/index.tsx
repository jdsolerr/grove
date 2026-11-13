/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import { commands } from "./bindings";

render(() => <App />, document.getElementById("root") as HTMLElement);
// render() es síncrono: el DOM ya está montado. No usar requestAnimationFrame: con la ventana oculta
// WKWebView no lo dispara y todo quedaba esperando la red de seguridad de 4 s en Rust.
commands.appReady().catch(() => {});
// Sin el menú nativo (Recargar/Inspeccionar); solo se deja en campos de texto para copiar/pegar.
// Las herramientas de desarrollo siguen con Cmd/Ctrl+Mayús+I en builds de desarrollo.
document.addEventListener("contextmenu", (e) => {
  if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) e.preventDefault();
});
