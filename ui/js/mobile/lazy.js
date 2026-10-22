// The phone's own screens (Home, the Library hub, Profile) are built the first
// time the phone layout shows them. A desktop window never has them in its DOM,
// so the wide layout stays exactly what it was.

import { h } from "../dom.js";
import { own, withScope } from "../store.js";

export function lazyView(factory, { methods = [] } = {}) {
  const element = h("section", { hidden: true });
  let real = null;
  let dispose = null;
  const ensure = () => {
    if (real) return real;
    ({ value: real, dispose } = withScope(factory));
    element.className = real.element.className;
    element.append(...real.element.childNodes);
    return real;
  };
  own(() => dispose?.());
  const view = { element, ensure, get built() { return !!real; } };
  for (const name of ["show", "reload", "reset", "root", "open", "setProfile", "embed", "navActions", ...methods]) {
    view[name] = (...args) => {
      // Housekeeping for a screen that was never shown has nothing to do.
      if (!real && (["reset", "setProfile", "root", "reload"].includes(name) || (name === "embed" && args[0] === false))) return undefined;
      const target = ensure();
      return target[name]?.(...args);
    };
  }
  view.navActions = () => (real ? real.navActions?.() ?? [] : []);
  return view;
}
