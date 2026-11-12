// A status is a dot and a word, never colour alone (ADR 0051 §4).

export type StatusState = "running" | "attention" | "stopped" | "failed" | "busy";

const words: Record<StatusState, string> = {
  running: "Running",
  attention: "Needs attention",
  stopped: "Stopped",
  failed: "Failed",
  busy: "Working…",
};

export function Dot({ state }: { state: StatusState }) {
  const look: Record<StatusState, string> = {
    running: "bg-ok",
    attention: "bg-warn",
    failed: "bg-bad",
    stopped: "border-[1.5px] border-off",
    busy: "border-[1.5px] border-off border-t-transparent animate-[spin_.9s_linear_infinite]",
  };
  return <span aria-hidden="true" className={`inline-block size-[7px] shrink-0 rounded-full forced-colors:border forced-colors:border-[CanvasText] ${look[state]}`} />;
}

export function Status({ state, label }: { state: StatusState; label?: string }) {
  const tone: Record<StatusState, string> = {
    running: "text-text",
    attention: "text-warn font-medium",
    failed: "text-bad font-medium",
    stopped: "text-text-3",
    busy: "text-text-2",
  };
  return (
    <span className={`inline-flex items-center gap-[7px] whitespace-nowrap text-[12.5px] ${tone[state]}`}>
      <Dot state={state} />
      {label ?? words[state]}
    </span>
  );
}
