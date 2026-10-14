// DOM helpers for the live views, which build their elements once and
// update them per frame outside jq79's reactivity (ADR 0005).

/** Creates an element with a class (and text), appended to `parent`. */
export const element = <K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  parent: Element,
  text?: string,
): HTMLElementTagNameMap[K] => {
  const created = document.createElement(tag);
  created.className = className;
  if (text !== undefined) created.textContent = text;
  parent.append(created);
  return created;
};

/** Sets an element's text only when it changed. */
export const setText = (target: Element, text: string): void => {
  if (target.textContent !== text) target.textContent = text;
};
