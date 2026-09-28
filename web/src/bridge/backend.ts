// Host-backed store: Gmail data and every write-back.
//
// The reducer stays the single source of truth so components are unchanged
// whether the app runs standalone on mock data or inside Electron. This module
// is the seam: it pulls snapshots from the host, pushes the write-backs the
// Rust app made (star, archive, mark read, label modify), and keeps the local
// store in step with push events.
import { useCallback, useEffect, useRef } from "react";
import { getNoriBridge, type NoriBridge } from "./noriBridge";
import type { LabelId } from "../state/store";
import type { DraftSeed, Email, EmailId } from "../types/mail";

export function isElectron(): boolean {
  return getNoriBridge() != null;
}

export interface HostBackend {
  /** True when the host bridge is present and mail comes from Gmail. */
  live: boolean;
  refresh: () => Promise<void>;
  /** Clear UNREAD. Only called for mail the user has not read. */
  markRead: (id: EmailId) => void;
  star: (id: EmailId, starred: boolean) => void;
  /** Remove INBOX. Archiving is that and nothing more. */
  archive: (id: EmailId) => void;
  /** Write a label toggle back, given the mail's current Gmail labels. */
  applyLabels: (
    id: EmailId,
    addedRemote: string[],
    removedRemote: string[],
  ) => void;
  fetchBody: (id: EmailId) => Promise<string[]>;
  search: (query: string) => Promise<Email[]>;
  send: (draft: DraftSeed) => Promise<void>;
  signin: () => Promise<void>;
  signout: () => Promise<void>;
  sync: () => Promise<void>;
}

type Dispatch = (action: unknown) => void;

/** IPC rejections arrive as "Error invoking remote method ... : <reason>". */
function describe(err: unknown): string {
  const message = err instanceof Error ? err.message : String(err);
  const colon = message.lastIndexOf(": ");
  return colon === -1 ? message : message.slice(colon + 2);
}

/**
 * Wire the host to the store. Returns the backend for imperative calls, and
 * pulls a snapshot on mount and whenever the host says something changed.
 */
export function useHostBackend(dispatch: Dispatch): HostBackend {
  const bridgeRef = useRef<NoriBridge | null>(getNoriBridge());
  const live = bridgeRef.current != null;

  const refresh = useCallback(async () => {
    const bridge = bridgeRef.current;
    if (!bridge?.snapshot) return;
    const snap = await bridge.snapshot();
    // Local label ids are the snapshot's own order; the reducer holds the
    // remote-to-local mapping, so only the forward direction travels.
    const localToRemote: Record<LabelId, string> = {};
    snap.labels.forEach((label, index) => {
      localToRemote[(index + 1) as LabelId] = label.id;
    });
    dispatch({
      type: "load-snapshot",
      // No cast: the host and the renderer agree on the mail shape, and a
      // cast here is what let a missing `body` reach the reading view.
      emails: snap.emails,
      labels: snap.labels.map((label, index) => ({
        id: (index + 1) as LabelId,
        name: label.name,
        colour: label.colour,
      })),
      assignments: snap.assignments,
      remoteLabels: Object.fromEntries(snap.emails.map((e) => [e.id, e.labelIds ?? []])),
      remoteLabelIds: localToRemote,
    });
    dispatch({ type: "set-syncing", syncing: snap.syncing });
    dispatch({
      type: "set-account",
      status: snap.account ? "connected" : "disconnected",
      address: snap.account,
    });
  }, [dispatch]);

  useEffect(() => {
    const bridge = bridgeRef.current;
    if (!bridge) return;
    let cancelled = false;
    refresh().catch(() => undefined);
    const unsubscribe = bridge.subscribe((event) => {
      if (cancelled) return;
      if (event.type === "emails-changed" || event.type === "account-changed") {
        refresh().catch(() => undefined);
      }
    });
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [refresh]);

  const modify = useCallback(
    (id: EmailId, add: string[], remove: string[]) =>
      bridgeRef.current?.modify?.(id, add, remove),
    [],
  );

  return {
    live,
    refresh,
    markRead(id) {
      void modify(id, [], ["UNREAD"]);
    },
    star(id, starred) {
      void modify(id, starred ? ["STARRED"] : [], starred ? [] : ["STARRED"])?.then((settled) =>
        dispatch({ type: "apply-labels", id, ...settled }),
      );
    },
    archive(id) {
      void modify(id, [], ["INBOX"]);
    },
    applyLabels(id, addedRemote, removedRemote) {
      void modify(id, addedRemote, removedRemote);
    },
    async fetchBody(id) {
      const body = (await bridgeRef.current?.fetchBody?.(id)) ?? [];
      if (body.length > 0) dispatch({ type: "load-body", id, body });
      return body;
    },
    async search(query) {
      const bridge = bridgeRef.current;
      if (!bridge?.search) return [];
      // The search endpoint answers with summaries; the reading view fills
      // the rest in from the snapshot when one of them is opened.
      return (await bridge.search(query)) as unknown as Email[];
    },
    async send(draft) {
      await bridgeRef.current?.send(draft);
    },
    async signin() {
      const bridge = bridgeRef.current;
      // No bridge means there is nothing to sign in to. Saying "fetching"
      // here is what left the page claiming to be signed in while no
      // browser ever opened.
      if (!bridge?.signin) {
        dispatch({
          type: "set-account",
          status: "failed",
          address: null,
          reason: "This build has no Gmail connection. Run the Electron app to sign in.",
        });
        return;
      }
      dispatch({ type: "set-account", status: "connecting" });
      try {
        const address = await bridge.signin();
        dispatch({ type: "set-account", status: "fetching", address });
        await refresh();
      } catch (err) {
        // Every failure lands here with the reason, so the page can name it
        // rather than spinning forever on "Fetching your mail".
        dispatch({
          type: "set-account",
          status: "failed",
          address: null,
          reason: describe(err),
        });
      }
    },
    async signout() {
      await bridgeRef.current?.signout?.();
      dispatch({ type: "set-account", status: "disconnected", address: null });
    },
    async sync() {
      await bridgeRef.current?.sync?.();
      await refresh();
    },
  };
}
