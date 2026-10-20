/**
 * The signal a tab panel sends to a tab it has just put back.
 *
 * A panel parks a hidden tab with `remove()`. A streaming child stops in
 * `disconnectedCallback`, because a detached element must not keep an SSE
 * stream or a socket open. Restoring it is `append()`, which changes no
 * property, so `willUpdate` does not run: without an explicit signal the child
 * would stay stopped while it is visible again.
 *
 * The event is dispatched on the tab element itself, so a child subscribes
 * without the panel knowing what kind of content it holds.
 */

export const ANTH_TAB_SHOW_EVENT = 'anth-tab-show'

/**
 * Calls `handler` when `target` is shown again by a tab panel.
 *
 * Returns the unsubscribe function; call it from `disconnectedCallback`, or a
 * component that is moved will keep a listener on the old element.
 */
export function onTabShow(
  target: EventTarget,
  handler: () => void,
): () => void {
  target.addEventListener(ANTH_TAB_SHOW_EVENT, handler)
  return () => target.removeEventListener(ANTH_TAB_SHOW_EVENT, handler)
}
