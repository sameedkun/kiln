import "@fontsource-variable/inter";
import "@fontsource/cascadia-mono/400.css";
import "./styles/app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { startAppearance, type Appearance } from "./appearance";
import { ToastProvider } from "./components/Toast";
import { Gallery } from "./gallery/Gallery";

// Until the app's shell lands (P4-03), the page is the design gallery. `?theme=dark` and
// `?solid=1` pick an appearance, for the screenshot and accessibility checks.
const params = new URLSearchParams(location.search);
const theme = params.get("theme");
startAppearance({
  theme: theme === "light" || theme === "dark" ? theme : "system",
  solid: params.has("solid") ? true : "system",
} satisfies Partial<Appearance>);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastProvider>
      <Gallery />
    </ToastProvider>
  </StrictMode>,
);
