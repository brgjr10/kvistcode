// The New agent dialog's small rules, kept apart so they can be tested.
import type { Provider } from "./types";

/**
 * Why Launch can't be pressed yet, in plain words (null when it can). Mirrors the dialog's
 * `canSubmit`, so the disabled button always says what is missing.
 */
export function launchBlocker(s: {
  project: string;
  compare: boolean;
  /** How many targets are picked (accounts or custom agents). */
  chosen: number;
  /** Agents already started by an earlier, partly failed launch in this dialog. */
  launched: number;
  prompt: string;
  provider: Provider;
  model: string;
  notGit: boolean;
}): string | null {
  if (!s.project.trim()) return "Choose a project folder";
  if (s.compare) {
    if (s.chosen < 2 && s.launched === 0) return "Pick at least two agents to compare";
    if (s.chosen === 0) return "Pick the agents to start";
    if (!s.prompt.trim()) return "Write the prompt to compare";
    return null;
  }
  if (s.chosen === 0) return s.provider === "local" ? "Add a local model server" : "Choose an account";
  if (s.notGit) return "Several agents need a git repository";
  if (s.provider === "local" && !s.model.trim()) return "Choose a model";
  return null;
}

/**
 * A prompt typed into the dialog and closed without launching comes back the next time it opens
 * (memory only, like an unsent message), unless that opening brings its own prompt.
 */
let unsent = "";

export function keepUnsentPrompt(prompt: string) {
  unsent = prompt.trim() ? prompt : "";
}

/** The prompt to start the dialog with: the preset's, else the one left unsent last time. */
export function initialPrompt(preset: string | undefined): string {
  if (preset !== undefined && preset !== "") return preset;
  return unsent;
}
