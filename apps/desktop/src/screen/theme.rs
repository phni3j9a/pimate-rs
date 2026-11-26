//! 色・角丸・寸法と部品の置き場（docs/design/desktop）。
//! 画面のコードでは色と角丸を直接書かず、ここのものを使う。
use super::*;
use std::borrow::Cow;

pub(super) const SIDEBAR: f32 = 264.;
pub(super) const SIDEBAR_RANGE: (f32, f32) = (220., 360.);
pub(super) const COLUMN: f32 = 720.;
pub(super) const HEADER: f32 = 52.;
pub(super) const ROW: f32 = 48.;
pub(super) const CONTROL: f32 = 36.;
/// 角丸。行=10、ボタン=8、カード・ダイアログ=16、入力欄と主ボタン=カプセル。
pub(super) const R_ROW: f32 = 10.;
pub(super) const R_CONTROL: f32 = 8.;
pub(super) const R_CARD: f32 = 16.;
pub(super) const R_KBD: f32 = 5.;
pub(super) const R_COMPOSER: f32 = 20.;

/// ウィンドウの幅ごとの並べ方（README「並べ方」）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Layout {
    /// 1080px 以上: サイドバー＋会話＋インスペクター。
    Wide,
    /// 760〜1079px: サイドバー＋会話。インスペクターは重ねる。
    Medium,
    /// 760px 未満: 会話だけ。サイドバーは重ねて出す。
    Narrow,
}
impl Layout {
    pub(super) fn of(width: Pixels) -> Self {
        if width >= px(1080.) {
            Self::Wide
        } else if width >= px(760.) {
            Self::Medium
        } else {
            Self::Narrow
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)] // 差分・リング・完了の色は段階3・4の画面で使う。
pub(super) struct Colors {
    pub keyboard_focus: bool,
    pub bg: Hsla,
    /// 塗り（サイドバー・入力欄・カード）。
    pub surface: Hsla,
    /// 塗り濃（ホバー・区切り線・kbd）。
    pub fill2: Hsla,
    /// 選択。
    pub bubble: Hsla,
    pub ink: Hsla,
    pub secondary: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub accent_ink: Hsla,
    pub on_accent: Hsla,
    /// 入力待ちの文字。印は `wait`。
    pub waiting: Hsla,
    pub wait: Hsla,
    pub fail: Hsla,
    pub fail_ink: Hsla,
    pub done: Hsla,
    pub add_bg: Hsla,
    pub add_ink: Hsla,
    pub del_bg: Hsla,
    pub del_ink: Hsla,
    pub track: Hsla,
    pub scrim: Hsla,
}
impl Colors {
    pub(super) fn new(dark: bool) -> Self {
        let c = |light, d| -> Hsla { rgb(if dark { d } else { light }).into() };
        let fill2 = c(0xe6e9eb, 0x293035);
        Self {
            keyboard_focus: false,
            bg: c(0xffffff, 0x131618),
            surface: c(0xf1f3f4, 0x1e2326),
            fill2,
            bubble: c(0xdfe7ec, 0x293a43),
            ink: c(0x1f2326, 0xe4e8ea),
            secondary: c(0x5e686d, 0xa8b2b8),
            border: fill2,
            accent: c(0x1b6b91, 0x88c8e6),
            accent_ink: c(0x155a7a, 0x88c8e6),
            on_accent: c(0xffffff, 0x10242e),
            waiting: c(0x8b5d20, 0xf1bd68),
            wait: c(0xe1ad42, 0xf1bd68),
            fail: c(0xe37769, 0xf0958a),
            fail_ink: c(0xa84d45, 0xf0958a),
            done: c(0x4d9abf, 0x6bb4d6),
            add_bg: c(0xe8f3f9, 0x173340),
            add_ink: c(0x22607e, 0x9fd3ea),
            del_bg: c(0xfbedeb, 0x3a2523),
            del_ink: c(0x9a453e, 0xf0a69c),
            track: c(0xc5cdd1, 0x3b454b),
            scrim: if dark {
                gpui::black().opacity(0.5)
            } else {
                c(0x1f2326, 0).opacity(0.28)
            },
        }
    }
    fn variant(&self, bg: Hsla, fg: Hsla, hover: Hsla, cx: &App) -> ButtonCustomVariant {
        ButtonCustomVariant::new(cx)
            .color(bg)
            .foreground(fg)
            .hover(hover)
            .active(self.bubble)
    }
    /// 塗りの主ボタン（カプセル型）。1画面に1つ。
    pub(super) fn primary(&self, id: &'static str, cx: &App) -> Button {
        Button::new(id)
            .when(self.keyboard_focus, |b| {
                b.focus(|s| s.border_2().border_color(self.accent))
            })
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(self.accent)
                    .foreground(self.on_accent)
                    .hover(self.accent.opacity(0.9))
                    .active(self.accent.opacity(0.8)),
            )
            // 文字の指定を足すと Button の文字色が消えるので、大きさは small に任せる。
            .small()
            .rounded(px(CONTROL / 2.))
            .h(px(CONTROL))
            .px_4()
    }
    /// 文字ボタン。`danger` は終了・削除・停止・取り消し。
    pub(super) fn text_button(&self, id: impl Into<ElementId>, color: Hsla, cx: &App) -> Button {
        Button::new(id)
            .when(self.keyboard_focus, |b| {
                b.focus(|s| s.border_2().border_color(self.accent))
            })
            .custom(self.variant(gpui::transparent_black(), color, self.fill2, cx))
            .small()
            .rounded(px(R_CONTROL))
            .h(px(32.))
    }
    /// アイコンだけのボタン。ツールチップを必ず付ける。
    pub(super) fn icon_button(
        &self,
        id: impl Into<ElementId>,
        name: &'static str,
        tooltip: &'static str,
        cx: &App,
    ) -> Button {
        self.text_button(id, self.secondary, cx)
            .size(px(32.))
            .flex_none()
            .tooltip(tooltip)
            .child(icon(name, 16., self.secondary))
    }
    /// 一覧の行。ホバーは塗り濃、選択は選択色。
    pub(super) fn row(&self, id: impl Into<ElementId>, selected: bool, cx: &App) -> Button {
        let bg = if selected {
            self.bubble
        } else {
            gpui::transparent_black()
        };
        let hover = if selected { self.bubble } else { self.fill2 };
        Button::new(id)
            .when(self.keyboard_focus, |b| {
                b.focus(|s| s.border_2().border_color(self.accent))
            })
            .custom(self.variant(bg, self.ink, hover, cx))
            .small()
            .rounded(px(R_ROW))
            .w_full()
            .justify_start()
    }
    pub(super) fn caption(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(rems(0.75))
            .text_color(self.secondary)
            .child(text.into())
    }
    /// 状態の印（7px の丸）。
    pub(super) fn dot(&self, color: Hsla) -> Div {
        div().size(px(7.)).flex_none().rounded_full().bg(color)
    }
    /// 浮く面（ダイアログ・重ねたサイドバー）の背景を暗くする幕。
    pub(super) fn scrim(&self) -> Div {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .bg(self.scrim)
    }
    pub(super) fn card(&self) -> Div {
        div().bg(self.surface).rounded(px(R_CARD)).px_4().py_3()
    }
}
/// 段階2以降の画面（メニュー・設定・ショートカット）で使う部品。
#[allow(dead_code)]
impl Colors {
    pub(super) fn kbd(&self, keys: &'static str) -> Div {
        div()
            .flex_none()
            .px_1()
            .rounded(px(R_KBD))
            .bg(self.fill2)
            .text_color(self.secondary)
            .text_size(rems(0.69))
            .font_family("monospace")
            .child(keys)
    }
    /// セグメント（どれか1つを選ぶ）。
    pub(super) fn segment(&self, items: impl IntoIterator<Item = (AnyElement, bool)>) -> Div {
        div()
            .flex()
            .p_0p5()
            .rounded(px(R_ROW))
            .bg(self.fill2)
            .children(items.into_iter().map(|(item, on)| {
                div()
                    .px_3()
                    .py_1()
                    .rounded(px(R_CONTROL))
                    .text_color(self.secondary)
                    .when(on, |d| {
                        d.bg(self.bg)
                            .text_color(self.ink)
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .child(item)
            }))
    }
    /// メニューの行（高さ 34px）。
    pub(super) fn menu_row(&self, id: impl Into<ElementId>, cx: &App) -> Button {
        self.row(id, false, cx).rounded(px(R_CONTROL)).h(px(34.))
    }
}

/// 会話の列。幅 720px までを中央に置く（行の左右の余白 16px ぶんを足す）。
pub(super) fn column(child: impl IntoElement) -> Div {
    div().w_full().flex().justify_center().child(
        div()
            .w_full()
            .max_w(px(COLUMN + 32.))
            .min_w_0()
            .debug_selector(|| "chat-column".into())
            .child(child),
    )
}

/// アイコンキットの線画（24px の格子、線 1.7）。色は文字色に従う。
const ICONS: &[(&str, &str)] = &[
    (
        "search",
        r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
    ),
    ("x", r#"<path d="m6 6 12 12M18 6 6 18"/>"#),
    ("plus", r#"<path d="M12 5v14M5 12h14"/>"#),
    (
        "gear",
        r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9 7 7M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"/>"#,
    ),
    (
        "github",
        r#"<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M18 15.5V10a4 4 0 0 0-4-4h-3"/>"#,
    ),
    (
        "diff",
        r#"<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M9 9h6M12 6v6M9 16h6"/>"#,
    ),
    (
        "copy",
        r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h9"/>"#,
    ),
    (
        "laptop",
        r#"<rect x="5" y="5" width="14" height="10" rx="1.5"/><path d="M3 19h18"/>"#,
    ),
    (
        "side",
        r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>"#,
    ),
    (
        "bell",
        r#"<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 21h4"/>"#,
    ),
    // gpui-component が使う名前: 入力欄の消去ボタンと TitleBar。
    (
        "circle-x",
        r#"<circle cx="12" cy="12" r="9"/><path d="m9 9 6 6M15 9l-6 6"/>"#,
    ),
    ("window-close", r#"<path d="M6 6l12 12M18 6 6 18"/>"#),
    ("window-minimize", r#"<path d="M5 12h14"/>"#),
    (
        "window-maximize",
        r#"<rect x="5" y="5" width="14" height="14" rx="1.5"/>"#,
    ),
    (
        "window-restore",
        r#"<rect x="5" y="8" width="11" height="11" rx="1.5"/><path d="M8 8V5h11v11h-3"/>"#,
    ),
];
/// svg は親の文字色を継がないので、色は必ず渡す。
pub(super) fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}
pub(crate) struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .and_then(|s| s.strip_suffix(".svg"));
        Ok(ICONS.iter().find(|(n, _)| Some(*n) == name).map(|(_, body)| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
            )
            .into_bytes()
            .into()
        }))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(n, _)| format!("icons/{n}.svg").into())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{Assets, ICONS, Layout};
    use gpui::{AssetSource, px};
    #[test]
    fn layout_breaks_at_1080_and_760() {
        for (width, layout) in [
            (1280., Layout::Wide),
            (1080., Layout::Wide),
            (1079., Layout::Medium),
            (760., Layout::Medium),
            (759., Layout::Narrow),
            (480., Layout::Narrow),
        ] {
            assert_eq!(Layout::of(px(width)), layout, "{width}");
        }
    }
    #[test]
    fn every_icon_is_served_as_svg() {
        for (name, _) in ICONS {
            let svg = Assets.load(&format!("icons/{name}.svg")).unwrap().unwrap();
            assert!(svg.starts_with(b"<svg") && svg.ends_with(b"</svg>"));
        }
        assert!(Assets.load("icons/unknown.svg").unwrap().is_none());
    }
}
