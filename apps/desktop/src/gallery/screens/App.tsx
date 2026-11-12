import {
  Code20Regular,
  Cube20Regular,
  Database20Regular,
  Globe20Regular,
  History20Regular,
  Home20Regular,
  Settings20Regular,
  TextBulletListLtr20Regular,
} from "@fluentui/react-icons";
import { useState } from "react";
import { Sidebar, type NavItem } from "../../components/Sidebar";
import { Dot } from "../../components/Status";
import { TitleBar } from "../../components/TitleBar";
import { History } from "./History";
import { Home } from "./Home";
import { Logs } from "./Logs";
import { Node, Php } from "./Runtimes";
import { Services } from "./Services";
import { Settings } from "./Settings";
import { Site } from "./Site";
import { Sites } from "./Sites";

const pages: NavItem[] = [
  { id: "home", label: "Home", icon: <Home20Regular /> },
  { id: "sites", label: "Sites", icon: <Globe20Regular />, detail: "8" },
  { id: "services", label: "Services", icon: <Database20Regular />, attention: true },
  { id: "php", label: "PHP", icon: <Code20Regular />, detail: "8.5 · 8.4 · 8.3" },
  { id: "node", label: "Node", icon: <Cube20Regular />, detail: "24 · 22" },
  { id: "logs", label: "Logs", icon: <TextBulletListLtr20Regular />, groupStart: true },
  { id: "history", label: "History", icon: <History20Regular /> },
];
const footer: NavItem[] = [{ id: "settings", label: "Settings", icon: <Settings20Regular /> }];

// The app's window with sample data: every page, clickable. P4-03 puts the real shell
// around these screens, and P4-04 to P4-08 connect them to `kilnd`.
export function App({ start = "home" }: { start?: string }) {
  const [page, setPage] = useState(start);
  const section = page.startsWith("site:") ? "sites" : page;
  return (
    <div className="backdrop relative grid h-[720px] w-[1120px] shrink-0 grid-rows-[44px_1fr] overflow-hidden rounded-lg shadow-[0_2px_8px_rgba(0,0,0,.18),0_30px_80px_rgba(0,0,0,.35)] outline outline-1 outline-black/25">
      <TitleBar />
      <div className="grid min-h-0 grid-cols-[220px_1fr] gap-2 px-2 pb-2">
        <Sidebar
          items={pages}
          footer={footer}
          current={section}
          onSelect={setPage}
          status={
            <>
              <Dot state="running" />
              Web server running on :443
            </>
          }
        />
        <Screen key={page} page={page} go={setPage} />
      </div>
    </div>
  );
}

function Screen({ page, go }: { page: string; go: (p: string) => void }) {
  if (page.startsWith("site:")) return <Site name={page.slice(5)} go={go} />;
  switch (page) {
    case "sites":
      return <Sites go={go} />;
    case "services":
      return <Services />;
    case "php":
      return <Php />;
    case "node":
      return <Node />;
    case "logs":
      return <Logs />;
    case "history":
      return <History />;
    case "settings":
      return <Settings />;
    default:
      return <Home go={go} />;
  }
}
