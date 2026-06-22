// @vitest-environment happy-dom
import { describe, it, expect, vi, afterEach } from "vitest";
import { dismissable } from "./dismiss";

afterEach(() => {
  document.body.innerHTML = "";
});

function setup(opts: { ignore?: HTMLElement } = {}) {
  const menu = document.createElement("div");
  const outside = document.createElement("div");
  document.body.append(menu, outside);
  const onDismiss = vi.fn();
  const action = dismissable(menu, { onDismiss, ignore: opts.ignore });
  return { menu, outside, onDismiss, action };
}

const pointerdown = (el: EventTarget) => el.dispatchEvent(new Event("pointerdown", { bubbles: true }));

describe("dismissable", () => {
  it("dismisses on an outside pointerdown", () => {
    const { outside, onDismiss, action } = setup();
    pointerdown(outside);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    action.destroy();
  });

  it("ignores a pointerdown inside the node", () => {
    const { menu, onDismiss, action } = setup();
    const child = menu.appendChild(document.createElement("button"));
    pointerdown(child);
    expect(onDismiss).not.toHaveBeenCalled();
    action.destroy();
  });

  it("treats the `ignore` element (the trigger) as inside, so it can toggle", () => {
    const trigger = document.body.appendChild(document.createElement("button"));
    const { onDismiss, action } = setup({ ignore: trigger });
    pointerdown(trigger);
    expect(onDismiss).not.toHaveBeenCalled();
    action.destroy();
  });

  it("dismisses on Escape", () => {
    const { onDismiss, action } = setup();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    action.destroy();
  });

  it("ignores non-Escape keys", () => {
    const { onDismiss, action } = setup();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    expect(onDismiss).not.toHaveBeenCalled();
    action.destroy();
  });

  it("dismisses on an outer scroll but not a scroll within the node", () => {
    const { menu, onDismiss, action } = setup();
    document.dispatchEvent(new Event("scroll")); // target = document (outside)
    expect(onDismiss).toHaveBeenCalledTimes(1);
    menu.dispatchEvent(new Event("scroll", { bubbles: true })); // inside
    expect(onDismiss).toHaveBeenCalledTimes(1); // unchanged
    action.destroy();
  });

  it("dismisses on window resize", () => {
    const { onDismiss, action } = setup();
    window.dispatchEvent(new Event("resize"));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    action.destroy();
  });

  it("stops listening after destroy", () => {
    const { outside, onDismiss, action } = setup();
    action.destroy();
    pointerdown(outside);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    window.dispatchEvent(new Event("resize"));
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("uses the latest callback after update()", () => {
    const { outside, action } = setup();
    const next = vi.fn();
    action.update({ onDismiss: next });
    pointerdown(outside);
    expect(next).toHaveBeenCalledTimes(1);
    action.destroy();
  });
});
