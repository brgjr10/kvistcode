/** Controls that act on Enter themselves: a link, a tab, a disclosure, a role="button" card. */
const OWN_ENTER = "button, a[href], [role=button], [role=link], [role=tab], [role=radio], [role=menuitem], summary";

/**
 * Whether Enter belongs to the focused element. The Needs you pane opens the agent on Enter; doing that
 * while a link in the agent's answer had focus swallowed the link (and any other control's own Enter).
 */
export function keepsOwnEnter(target: EventTarget | null): boolean {
  const el = target as { closest?: (selector: string) => unknown } | null;
  return typeof el?.closest === "function" && el.closest(OWN_ENTER) != null;
}
