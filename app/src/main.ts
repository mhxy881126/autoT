import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./styles.css";

const app = createApp(App);
const showError = (err: unknown) => {
  const text = err instanceof Error ? `${err.message}\n${err.stack ?? ""}` : String(err);
  const el = document.createElement("pre");
  el.style.cssText =
    "position:fixed;inset:0;z-index:99999;background:#111;color:#ff8a87;padding:20px;overflow:auto;font:12px Consolas,monospace;white-space:pre-wrap";
  el.textContent = `前端运行错误\n\n${text}`;
  document.body.appendChild(el);
};
app.config.errorHandler = (err) => showError(err);
window.addEventListener("error", (e) => showError(e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) => showError(e.reason));

app.use(createPinia()).mount("#app");
