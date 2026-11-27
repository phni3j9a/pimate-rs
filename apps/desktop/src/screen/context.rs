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
            app.update(cx, |app, cx| app.show_context(menu, position, w, cx));
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
impl Desktop {
    pub(super) fn show_context(
        &mut self,
        menu: Entity<PopupMenu>,
        position: Point<Pixels>,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.weak_entity();
        let dismiss = w.subscribe(&menu, cx, move |menu, _: &DismissEvent, w, cx| {
            let _ = app.update(cx, |app, cx| {
                if app.context_menu.as_ref().is_some_and(|v| v.menu == menu) {
                    app.context_menu = None;
                    app.restore_input(w, cx);
                    cx.notify();
                }
            });
        });
        menu.focus_handle(cx).focus(w);
        self.push_overlay(crate::screen::keys::Overlay::Menu);
        self.context_menu = Some(OpenMenu {
            position,
            menu,
            _dismiss: dismiss,
        });
        cx.notify();
    }
}
// InputState's built-in mouse menu leaves focus in the editor. Use the same focused
// popup as the rest of the window, before the input's right-click listener runs.
pub(super) fn editor(row: Div, input: Entity<InputState>, disabled: bool) -> Div {
    row.capture_any_mouse_down(move |e, w, cx| {
        if e.button != MouseButton::Right {
            return;
        }
        let Some(app) = w
            .root::<Root>()
            .flatten()
            .and_then(|root| root.read(cx).view().clone().downcast::<Desktop>().ok())
        else {
            return;
        };
        cx.stop_propagation();
        w.prevent_default();
        let selected = input.update(cx, |s, cx| {
            s.selected_text_range(false, w, cx)
                .is_some_and(|r| !r.range.is_empty())
        });
        let focus = input.focus_handle(cx);
        let owner = app.downgrade();
        let menu = PopupMenu::build(w, cx, |m, _, _| {
            use gpui_component::input::*;
            m.action_context(focus.clone())
                .menu_with_enable("切り取り", Box::new(Cut), selected && !disabled)
                .menu_with_enable("コピー", Box::new(Copy), selected)
                .item(PopupMenuItem::new("貼り付け").disabled(disabled).on_click(
                    move |_, w, cx| {
                        let _ = owner.update(cx, |d, cx| {
                            if d.front(w, cx) != Some(keys::Overlay::Menu) {
                                return;
                            }
                            d.context_menu = None;
                            focus.focus(w);
                            w.dispatch_action(Box::new(Paste), cx);
                            cx.notify();
                        });
                    },
                ))
                .separator()
                .menu("すべてを選択", Box::new(SelectAll))
        });
        app.update(cx, |app, cx| app.show_context(menu, e.position, w, cx));
    })
}
pub(super) fn field(input: &Entity<InputState>, disabled: bool) -> Div {
    editor(div(), input.clone(), disabled).child(Input::new(input).disabled(disabled))
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
