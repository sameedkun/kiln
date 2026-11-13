import { useCallback, useEffect, useState } from "react";
import { call, daemonState, message, onDaemon, onEvent, onReconnected, type DaemonState } from "./api";

export interface Read<T> {
  data?: T;
  error?: string;
  loading: boolean;
  reload: () => void;
}

/** A read method's answer, read again whenever Kiln changes something or reconnects. */
export function useRead<T>(method: string, params?: unknown): Read<T> {
  const [state, setState] = useState<{ data?: T; error?: string; loading: boolean }>({ loading: true });
  const key = JSON.stringify(params ?? null);
  const load = useCallback(() => {
    call<T>(method, JSON.parse(key))
      .then((data) => setState({ data, loading: false }))
      .catch((e) => setState((s) => ({ ...s, error: message(e), loading: false })));
  }, [method, key]);
  useEffect(() => {
    load();
    let live = true;
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => (live ? stops.push(stop) : stop());
    onEvent((note) => note.method === "state.changed" && load()).then(keep);
    onReconnected(load).then(keep);
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, [load]);
  return { ...state, reload: load };
}

/** Whether `kilnd` answers, as the app's Rust side sees it. */
export function useDaemon(): DaemonState {
  const [state, setState] = useState<DaemonState>("starting");
  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    daemonState().then((s) => live && setState(s));
    onDaemon(setState).then((u) => (live ? (stop = u) : u()));
    return () => {
      live = false;
      stop?.();
    };
  }, []);
  return state;
}
