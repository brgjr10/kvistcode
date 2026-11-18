/**
 * What the "can't reach Kvistcode" screen says. A phone with no network at all is told so plainly:
 * checking the computer, Remote access and Wi-Fi pairing would only send the user the wrong way.
 */
import type { BlockedReason } from "./store";

export interface BlockedCopy {
  /** The phone itself has no network (only for an unreachable computer). */
  offline: boolean;
  /** Keep retrying in the background (and say so). */
  autoRetry: boolean;
  /** The line under the buttons, or null. */
  retryNote: string | null;
}

export function blockedCopy(reason: BlockedReason | null, online: boolean): BlockedCopy {
  const unreachable = reason === "unreachable";
  const offline = unreachable && !online;
  const autoRetry = unreachable || reason === "unavailable";
  return {
    offline,
    autoRetry,
    retryNote: !autoRetry ? null : offline ? "Reconnects by itself when your phone is back online." : "Retrying automatically…",
  };
}

/** One background attempt is worth making: visible page, a network to try on, nothing already out. */
export const shouldAutoRetry = (visible: boolean, online: boolean, trying: boolean) => visible && online && !trying;
