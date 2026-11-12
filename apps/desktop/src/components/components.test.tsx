import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setAppearance } from "../appearance";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { Menu } from "./Menu";
import { Sidebar } from "./Sidebar";
import { Status } from "./Status";
import { Switch } from "./Switch";
import { TextField } from "./TextField";
import { ToastProvider, useToast } from "./Toast";

afterEach(cleanup);

describe("Status", () => {
  it("always shows a word, not colour alone", () => {
    render(
      <>
        <Status state="running" />
        <Status state="attention" label="Stopped after a crash" />
      </>,
    );
    expect(screen.getByText("Running")).toBeTruthy();
    expect(screen.getByText("Stopped after a crash")).toBeTruthy();
  });
});

describe("Switch", () => {
  it("is a labelled switch that flips", async () => {
    function Wrapped() {
      const [on, setOn] = useState(false);
      return <Switch checked={on} onChange={setOn} label="Stay in the tray" />;
    }
    render(<Wrapped />);
    const sw = screen.getByRole("switch", { name: "Stay in the tray" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    await userEvent.click(sw);
    expect(sw.getAttribute("aria-checked")).toBe("true");
    sw.focus();
    await userEvent.keyboard(" ");
    expect(sw.getAttribute("aria-checked")).toBe("false");
  });
});

describe("TextField", () => {
  it("ties the label and the error to the input", () => {
    render(<TextField label="Port" defaultValue="3306" error="Port 3306 is in use." />);
    const input = screen.getByLabelText("Port");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    const described = input.getAttribute("aria-describedby")!;
    expect(document.getElementById(described)?.textContent).toBe("Port 3306 is in use.");
  });
});

describe("Menu", () => {
  function Example({ onUnlink }: { onUnlink: () => void }) {
    return (
      <Menu
        label="More for shop.test"
        trigger={(props) => <Button aria-label="More" {...props} />}
        items={[
          { label: "Open", onSelect: () => {} },
          { label: "Open the folder", onSelect: () => {} },
          { label: "Unlink", danger: true, onSelect: onUnlink },
        ]}
      />
    );
  }

  it("opens on the first item, moves with the arrows and wraps", async () => {
    render(<Example onUnlink={() => {}} />);
    const more = screen.getByRole("button", { name: "More" });
    await userEvent.click(more);
    expect(more.getAttribute("aria-expanded")).toBe("true");
    const items = screen.getAllByRole("menuitem");
    expect(document.activeElement).toBe(items[0]);
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(items[1]);
    await userEvent.keyboard("{ArrowUp}{ArrowUp}");
    expect(document.activeElement).toBe(items[2]);
  });

  it("closes on Esc and gives focus back to its button", async () => {
    render(<Example onUnlink={() => {}} />);
    const more = screen.getByRole("button", { name: "More" });
    await userEvent.click(more);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(more);
  });

  it("runs the chosen item and closes", async () => {
    const unlink = vi.fn();
    render(<Example onUnlink={unlink} />);
    await userEvent.click(screen.getByRole("button", { name: "More" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Unlink" }));
    expect(unlink).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("Dialog", () => {
  it("is named by its title and closes on Esc", async () => {
    const close = vi.fn();
    render(
      <Dialog open onClose={close} title="Unlink shop.test?" footer={<Button>Cancel</Button>}>
        The folder stays.
      </Dialog>,
    );
    expect(screen.getByRole("dialog", { name: "Unlink shop.test?" })).toBeTruthy();
    screen.getByRole("button", { name: "Cancel" }).focus();
    await userEvent.keyboard("{Escape}");
    expect(close).toHaveBeenCalled();
  });
});

describe("Toast", () => {
  it("is announced, offers its action and goes away", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const undo = vi.fn();
    function Show() {
      const toast = useToast();
      return <button onClick={() => toast({ message: "Linked api.test", action: { label: "Undo", onClick: undo } })}>show</button>;
    }
    render(
      <ToastProvider>
        <Show />
      </ToastProvider>,
    );
    await userEvent.click(screen.getByText("show"));
    expect(screen.getByRole("status").textContent).toContain("Linked api.test");
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(undo).toHaveBeenCalledOnce();
    await userEvent.click(screen.getByText("show"));
    act(() => {
      vi.advanceTimersByTime(7000);
    });
    expect(screen.getByRole("status").textContent).toBe("");
    vi.useRealTimers();
  });
});

describe("Sidebar", () => {
  it("marks the current page and says which page needs attention", async () => {
    const select = vi.fn();
    render(
      <Sidebar
        items={[
          { id: "home", label: "Home", icon: null },
          { id: "services", label: "Services", icon: null, attention: true },
        ]}
        footer={[]}
        current="home"
        onSelect={select}
      />,
    );
    expect(screen.getByRole("button", { name: "Home" }).getAttribute("aria-current")).toBe("page");
    const services = screen.getByRole("button", { name: /Services/ });
    expect(services.getAttribute("aria-current")).toBeNull();
    expect(screen.getByRole("img", { name: "needs attention" })).toBeTruthy();
    await userEvent.click(services);
    expect(select).toHaveBeenCalledWith("services");
  });
});

describe("appearance", () => {
  it("sets the theme and transparency on <html>", () => {
    setAppearance({ theme: "dark", solid: true });
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.hasAttribute("data-solid")).toBe(true);
    setAppearance({ theme: "light", solid: false });
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.hasAttribute("data-solid")).toBe(false);
  });
});
