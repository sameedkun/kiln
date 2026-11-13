import "@fontsource-variable/inter";
import "@fontsource/cascadia-mono/400.css";
import "../styles/app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { startAppearance } from "../appearance";
import { ToastProvider } from "../components/Toast";
import { inApp } from "./api";
import { ChangeProvider } from "./Change";
import { MainApp } from "./MainApp";
import { TrayApp } from "./TrayApp";

// The app's page (ADR 0054 §5): the main window, or the tray popup (`#tray`). Light, dark
// and transparency follow Windows as WebView2 reports them.
startAppearance();

function Outside() {
  return (
    <main className="backdrop grid h-screen place-items-center p-8 text-center text-[13px] text-text-2">
      <p className="m-0 max-w-[50ch]">
        This is Kiln's app, which runs inside its own window (<code className="font-mono">kiln-desktop.exe</code>). The design gallery is at{" "}
        <a className="text-text underline" href="gallery.html">
          gallery.html
        </a>
        .
      </p>
    </main>
  );
}

const tray = location.hash === "#tray";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastProvider>
      <ChangeProvider>{!inApp ? <Outside /> : tray ? <TrayApp /> : <MainApp />}</ChangeProvider>
    </ToastProvider>
  </StrictMode>,
);
