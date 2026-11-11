export type ConfirmOptions = {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
};

/// A native `<dialog>` confirmation. Risky actions (dropping an interface,
/// changing a route, signalling a process) must pass through this before the
/// service is called. Esc and the backdrop cancel; focus starts on Cancel.
export function confirmAction(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const dialog = document.createElement("dialog");
    dialog.className = "confirm-dialog";
    const form = document.createElement("form");
    form.method = "dialog";
    form.className = "confirm-card";

    const title = document.createElement("h3");
    title.textContent = options.title;
    const message = document.createElement("p");
    message.textContent = options.message;
    const actions = document.createElement("div");
    actions.className = "confirm-actions";
    const cancel = document.createElement("button");
    cancel.type = "submit";
    cancel.value = "cancel";
    cancel.className = "confirm-cancel";
    cancel.textContent = "Cancel";
    const confirm = document.createElement("button");
    confirm.type = "submit";
    confirm.value = "confirm";
    confirm.className = options.danger ? "confirm-ok danger" : "confirm-ok";
    confirm.textContent = options.confirmLabel ?? "Confirm";

    actions.append(cancel, confirm);
    form.append(title, message, actions);
    dialog.append(form);
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(dialog.returnValue === "confirm");
    });
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) dialog.close("cancel");
    });
    document.body.append(dialog);
    dialog.showModal();
    cancel.focus();
  });
}
