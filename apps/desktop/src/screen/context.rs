use super::*;
use gpui_component::menu::{PopupMenu, PopupMenuItem};

pub(crate) struct OpenMenu {
    position: Point<Pixels>,
    menu: Entity<PopupMenu>,
    _dismiss: Subscription,
}
pub(crate) fn copy(label: &'static str, text: String) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())))
}
// One popup per window: a code block nested inside a message must not open two menus.
pub(crate) fn on_right_click(row: Div, build: impl Fn(PopupMenu) -> PopupMenu + 'static) -> Div {
    let show = std::rc::Rc::new(
        move |position: Point<Pixels>, w: &mut Window, cx: &mut App| {
            let Some(root) = w.root::<Root>().flatten() else {
                return;
            };
            let Ok(app) = root.read(cx).view().clone().downcast::<Desktop>() else {
                return;
            };
            cx.stop_propagation();
            let previous = w.focused(cx);
            let menu = PopupMenu::build(w, cx, |m, _, _| {
                build(m.when_some(previous, |m, focus| m.action_context(focus)))
            });
            app.update(cx, |app, cx| {
                if app.confirmation.is_some() || app.palette_open {
                    return;
                }
                let dismiss = cx.subscribe(&menu, |app, menu, _: &DismissEvent, cx| {
                    if app.context_menu.as_ref().is_some_and(|v| v.menu == menu) {
                        app.context_menu = None;
                        cx.notify();
                    }
                });
                menu.focus_handle(cx).focus(w);
                app.context_menu = Some(OpenMenu {
                    position,
                    menu,
                    _dismiss: dismiss,
                });
                cx.notify();
            });
        },
    );
    let keys = show.clone();
    let anchor = std::rc::Rc::new(std::cell::Cell::new(Point::<Pixels>::default()));
    let read = anchor.clone();
    row.key_context("ContextTarget")
        .on_children_prepainted(move |bounds, _, _| {
            if let Some(b) = bounds.first() {
                anchor.set(b.bottom_left());
            }
        })
        .on_action(move |_: &super::keys::OpenContext, w, cx| keys(read.get(), w, cx))
        .on_mouse_down(MouseButton::Right, move |e, w, cx| show(e.position, w, cx))
}
impl OpenMenu {
    pub(super) fn layer(&self) -> AnyElement {
        deferred(
            anchored()
                .position(self.position)
                .snap_to_window_with_margin(px(8.))
                .child(self.menu.clone()),
        )
        .with_priority(1)
        .into_any_element()
    }
}
