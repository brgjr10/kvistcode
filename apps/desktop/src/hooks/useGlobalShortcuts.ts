import { useEffect } from "react";
import "./useShortcutMap";
import { shortcutFor, type ShortcutId } from "@/lib/shortcuts";
import { shortcutOrder } from "@/store/shortcutOrder";
import { onVaHotkey, onVaWake } from "@/lib/api";
import { selectNextTriage, triageQueueNow } from "@/components/triage/useTriage";
import { showOrHideAgentList } from "./useAgentListLayout";
import { isEditableTarget, isMac } from "@/lib/utils";
import { openShortcutHelp, toggleShortcutHelp } from "@/components/shell/ShortcutHelp";
import { stepInterfaceScale } from "@/lib/zoom";
import { useApp } from "@/store/app";
import { goBack, goForward } from "@/store/navHistory";
import { togglePreview } from "@/store/preview";
import { closeTour, useTour } from "@/store/tour";
import { useUi } from "@/store/ui";
import { toggleVoiceAssistant, wakeVoiceAssistant } from "@/store/voiceAssistant";

/** App-wide keyboard shortcuts. Mod = ⌘ on macOS, Ctrl elsewhere. */
export function useGlobalShortcuts(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      // ⌘ on macOS, Ctrl elsewhere (Ctrl+K on a Mac is a text-editing shortcut).
      const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      const action: ShortcutId | null = shortcutFor(e);
      // "?" (not while typing) opens the shortcut cheat sheet.
      if (!mod && e.key === "?" && !e.altKey && !e.repeat && !isEditableTarget(e.target) && !dialogOpen()) {
        e.preventDefault();
        openShortcutHelp();
        return;
      }
      const nav = action === "back" || action === "forward" ? action : null;
      if (nav && !e.repeat && !dialogOpen() && !inTerminal(e.target)) {
        e.preventDefault();
        if (nav === "back") goBack();
        else goForward();
        return;
      }
      // Alt+↑ / Alt+↓ (⌥ on a Mac) step to the previous / next agent in list order. Text fields,
      // terminals and the project header's own Alt+arrow nudge keep their keys.
      if (
        e.altKey &&
        !e.ctrlKey &&
        !e.metaKey &&
        !e.shiftKey &&
        (e.key === "ArrowUp" || e.key === "ArrowDown") &&
        !e.defaultPrevented &&
        !isEditableTarget(e.target) &&
        !inTerminal(e.target) &&
        !dialogOpen()
      ) {
        const list = shortcutOrder();
        if (list.length === 0) return;
        const step = e.key === "ArrowUp" ? -1 : 1;
        const at = list.findIndex((a) => a.id === useUi.getState().selectedAgentId);
        const next = at < 0 ? (step > 0 ? 0 : list.length - 1) : (at + step + list.length) % list.length;
        e.preventDefault();
        useUi.getState().openAgent(list[next].id);
        return;
      }
      // Dictation has its own listener (it must work while a message box has focus).
      // ⌘1–9 (jump to an agent) and the old ⌘I stay fixed.
      if (action === "dictate" || (!action && !(mod && !e.altKey && !e.shiftKey))) return;
      const ui = useUi.getState();
      const key = e.key.toLowerCase();

      if (action === "help") {
        e.preventDefault();
        toggleShortcutHelp();
        return;
      }

      // Interface zoom works anywhere, even over a dialog.
      if (action === "zoomIn") {
        e.preventDefault();
        stepInterfaceScale(1);
        return;
      }
      if (action === "zoomOut") {
        e.preventDefault();
        stepInterfaceScale(-1);
        return;
      }
      if (action === "zoomReset") {
        e.preventDefault();
        stepInterfaceScale(0);
        return;
      }

      if (action === "palette") {
        // The palette toggles itself; any other open dialog blocks it.
        if (!ui.paletteOpen && dialogOpen()) return;
        e.preventDefault();
        ui.setPaletteOpen(!ui.paletteOpen);
        return;
      }
      // The welcome tour teaches these two shortcuts, so they step it aside instead of being blocked by it.
      const tourOpen = useTour.getState().open;
      if (action === "search") {
        // Search every conversation (the palette's "?" mode); pressed again, it closes.
        if (!ui.paletteOpen && dialogOpen() && !tourOpen) return;
        if (tourOpen) closeTour();
        e.preventDefault();
        if (ui.paletteOpen) ui.setPaletteOpen(false);
        else ui.openSearch();
        return;
      }
      const newOverTour = action === "newAgent" && tourOpen;
      if (newOverTour) closeTour();
      // Ctrl+Shift+R is the webview's hard reload on Windows: it must never get through, dialog or not.
      if (action === "review") e.preventDefault();
      // Never navigate out from under an open dialog, sheet or menu.
      if (dialogOpen() && !newOverTour) return;
      if (action === "newAgent") {
        e.preventDefault();
        ui.setPaletteOpen(false);
        ui.openNewAgent();
        return;
      }
      if (action === "agentList") {
        e.preventDefault();
        showOrHideAgentList();
        return;
      }
      if (action === "workspace") {
        // Workspace panel (terminals, scripts) of the open agent.
        if (ui.view !== "agents" || !ui.selectedAgentId) return;
        e.preventDefault();
        ui.setWorkspaceOpen(!ui.workspaceOpen);
        return;
      }
      if (action === "preview") {
        // Preview pane (right of the conversation) of the open agent.
        if (ui.view !== "agents" || !ui.selectedAgentId) return;
        e.preventDefault();
        togglePreview(ui.selectedAgentId);
        return;
      }
      if (action === "review") {
        // Review the open agent's diff (agents with their own worktree).
        // Always handled: Ctrl+Shift+R is the webview's hard reload on Windows.
        e.preventDefault();
        const app = useApp.getState();
        const open = ui.selectedAgentId ? app.agents[ui.selectedAgentId] : undefined;
        if (ui.view !== "agents" || !open?.worktree) return;
        ui.setReviewAgent(open.id);
        return;
      }
      if (action === "needs") {
        e.preventDefault();
        // Opens Mission Control's "Needs you" tab; pressed again there, moves to the next waiting agent.
        if (ui.view === "mission" && ui.missionTab === "needs") selectNextTriage(triageQueueNow());
        else ui.openMission("needs");
        return;
      }
      if (!action && key === "i") {
        // The old Inbox's key: pending approvals are the first thing in "Needs you".
        e.preventDefault();
        ui.openMission("needs");
        return;
      }
      if (action === "voice") {
        e.preventDefault();
        toggleVoiceAssistant();
        return;
      }
      if (action === "mission") {
        e.preventDefault();
        ui.openMission("all");
        return;
      }
      if (action === "settings") {
        e.preventDefault();
        ui.openSettings();
        return;
      }
      if (!action && /^[1-9]$/.test(e.key)) {
        const list = shortcutOrder();
        const target = list[Number(e.key) - 1];
        if (target) {
          e.preventDefault();
          ui.openAgent(target.id);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);

  // The mouse's back and forward side buttons.
  useEffect(() => {
    if (!enabled) return;
    const onUp = (e: MouseEvent) => {
      if (e.button !== 3 && e.button !== 4) return;
      e.preventDefault();
      if (dialogOpen()) return;
      if (e.button === 3) goBack();
      else goForward();
    };
    window.addEventListener("mouseup", onUp);
    return () => window.removeEventListener("mouseup", onUp);
  }, [enabled]);

  // The system-wide voice assistant hotkey (works while the window is in the background):
  // the shell sends an event, and it does what the in-app shortcut does. The wake phrase
  // arrives the same way, with whatever was said after it.
  useEffect(() => {
    if (!enabled) return;
    const offs: (() => void)[] = [];
    let gone = false;
    const keep = (un: () => void) => {
      if (gone) un();
      else offs.push(un);
    };
    void onVaHotkey(toggleVoiceAssistant).then(keep);
    void onVaWake((e) => void wakeVoiceAssistant(e.request)).then(keep);
    return () => {
      gone = true;
      for (const off of offs) off();
    };
  }, [enabled]);
}

/** A shell uses Alt+arrows itself (word moves). */
export function inTerminal(t: EventTarget | null): boolean {
  return t instanceof Element && t.closest(".xterm") !== null;
}

export function dialogOpen(): boolean {
  return !!document.querySelector(
    '[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"], [role="menu"][data-state="open"]',
  );
}
