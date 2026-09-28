// Port of crates/nori-gmail/src/stream.rs — push-based incoming mail channel.
import type { RemoteMail } from "./sync.js";

export interface MailStream {
  push(mail: RemoteMail): void;
  finish(): void;
  subscribe(listener: (mail: RemoteMail) => void): () => void;
  drainInto(out: RemoteMail[]): boolean;
}

export function mailStream(): MailStream {
  const listeners = new Set<(mail: RemoteMail) => void>();
  const queue: RemoteMail[] = [];
  let open = true;
  return {
    push(mail: RemoteMail): void {
      if (!open) return;
      queue.push(mail);
      for (const listener of listeners) listener(mail);
    },
    finish(): void {
      open = false;
    },
    subscribe(listener: (mail: RemoteMail) => void): () => void {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    drainInto(out: RemoteMail[]): boolean {
      while (queue.length > 0) out.push(queue.shift() as RemoteMail);
      return open;
    },
  };
}
