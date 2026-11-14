// Hover hints are deliberately disabled: even a delayed title bubble can cover
// the query suggestions or the next control. Keep title text available to
// assistive technology without asking the browser to draw a floating bubble.
const descriptions = new WeakMap();
const labels = new WeakMap();
let initialized = false;

function suppressTitle(target) {
  if (!(target instanceof Element) || !target.hasAttribute("title")) return;
  const text = target.getAttribute("title").trim();
  if (text) {
    const previousDescription = descriptions.get(target);
    const description = target.getAttribute("aria-description");
    if (!description || description === previousDescription) {
      target.setAttribute("aria-description", text);
      descriptions.set(target, text);
    }

    // Some legacy icon buttons use title as their only accessible name. Give
    // those a durable name, without replacing visible labels or explicit ARIA.
    const previousLabel = labels.get(target);
    const label = target.getAttribute("aria-label");
    if (!target.hasAttribute("aria-labelledby") &&
        ((!label && !target.textContent.trim() &&
          target.matches('button, a[href], [role="button"], input[type="button"], input[type="submit"], input[type="reset"]')) ||
          (previousLabel && label === previousLabel))) {
      target.setAttribute("aria-label", text);
      labels.set(target, text);
    }
  } else if (target.getAttribute("aria-description") === descriptions.get(target)) {
    // Validation and progress controls clear their explanatory text explicitly.
    target.removeAttribute("aria-description");
    descriptions.delete(target);
  }
  target.removeAttribute("title");
}

function suppressTitles(root) {
  if (!(root instanceof Element)) return;
  suppressTitle(root);
  for (const target of root.querySelectorAll("[title]")) suppressTitle(target);
}

// Retain the controller API used when menus open; it never restores a title.
export function hideTooltip() {
  const tip = document.getElementById("tooltip");
  if (tip) tip.hidden = true;
}

export function initTooltips() {
  if (initialized) return;
  initialized = true;
  hideTooltip();
  suppressTitles(document.documentElement);

  // Menus, virtual rows, translations and button states all create or rewrite
  // titles after startup. Observe only titles and inserted nodes; no hover
  // timer, pointermove work or change to click/focus behavior is needed.
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === "attributes") suppressTitle(record.target);
      else for (const node of record.addedNodes) suppressTitles(node);
    }
  });
  observer.observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["title"],
  });
}
