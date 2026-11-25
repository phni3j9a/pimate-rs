use super::*;
use settings::{group, line, link};
use state::FirstStep;
fn bridge(p: &Colors) -> Div {
    let chip = |s| {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(p.border)
            .bg(p.bg)
            .child(s)
    };
    line()
        .p_3()
        .justify_center()
        .rounded_lg()
        .bg(p.surface)
        .text_color(p.accent)
        .child(chip("この端末"))
        .child("···")
        .child(chip("Tailscale"))
        .child("···")
        .child(chip("PC"))
}
impl Desktop {
    pub(super) fn guide(&self, n: &Navigation, p: &Colors, pc: bool, cx: &Context<Self>) -> Div {
        let root = div().flex().flex_col().gap_3().child(p.caption(if pc {
            "ペアリングの準備　2 / 3"
        } else {
            "ペアリングの準備　1 / 3"
        }));
        if !pc {
            return root
                .child(bridge(p))
                .child(
                    group("01　この端末", p)
                        .child("Tailscale にログインし、VPN の接続を許可してください。")
                        .child(link(
                            "tailscale",
                            "Tailscale をダウンロード ↗",
                            "https://tailscale.com/download",
                        )),
                )
                .child(
                    group("02　Pi を動かす PC", p)
                        .child("PC にも Tailscale を入れ、同じアカウントでログインします。")
                        .child(p.caption("Tailscale の端末一覧に PC が表示されれば準備完了です。")),
                )
                .child(self.nav_button(
                    n,
                    "guide-next",
                    "次へ：PC に PiMate を入れる　→",
                    Route::SetupPc,
                    cx,
                ));
        }
        let command = |id: &'static str, text: &'static str| {
            line()
                .justify_between()
                .p_2()
                .rounded_md()
                .bg(p.surface)
                .child(div().font_family("monospace").child(text))
                .child(
                    Button::new(id)
                        .small()
                        .ghost()
                        .label("コピー")
                        .tooltip(format!("{text} をコピー"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.into()))
                        }),
                )
        };
        root.child(group("01　PC のターミナルで実行",p).child("Rust 版の公開前は開発版パッケージを使ってください。公開後は次のコマンドで導入できます。").child(command("install-cmd","pi install npm:@phni3j9a/pimate")).child(command("pi-cmd","pi")))
            .child(group("02　Pi の入力欄で実行",p).child(command("setup-cmd","/pimate setup")).child(p.caption("表示されたペアリングリンクをコピーします。2台目や再登録には /pimate pair を使います。")))
            .child(p.primary("guide-paste",cx).label("リンクを貼り付ける").disabled(self.pairing).on_click(cx.listener(|this,_,_,cx|this.paste(cx))))
    }
    pub(super) fn first_step(
        &self,
        n: &Navigation,
        p: &Colors,
        id: &str,
        cx: &Context<Self>,
    ) -> Div {
        let host = self.hosts.iter().find(|h| h.id == id);
        let rows = self.machines.get(id).map(|m| &m.rows);
        let phase = if host.is_some_and(|h| h.needs_pairing)
            || self.state.registration.connection_errors.contains_key(id)
        {
            FirstStep::Offline
        } else {
            rows.map(Rows::first_step).unwrap_or(FirstStep::Loading)
        };
        let mut v = div().flex().flex_col().gap_3();
        match phase {
            FirstStep::Loading => return v.child("PC の Pi を確認しています…"),
            FirstStep::Offline => {
                return v.child("PC に接続していません。Tailscale と Host を確認してください。");
            }
            _ => {}
        }
        v = v.child(
            group("接続完了", p).child(
                div()
                    .text_size(rems(1.2))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!(
                        "{} とつながりました",
                        host.map(|h| self.host_label(h)).unwrap_or_default()
                    )),
            ),
        );
        if phase == FirstStep::Empty {
            v = v.child(bridge(p)).child(
                group("まだ起動中の Pi はありません", p)
                    .child("この端末から PC のフォルダを選んで、新しい Pi を起動できます。")
                    .child(
                        div()
                            .p_3()
                            .bg(p.surface)
                            .font_family("monospace")
                            .child("$ pi"),
                    )
                    .child(p.caption("PC のターミナルで起動した Pi も、ここに表示されます。")),
            );
        } else if let Some(rows) = rows {
            let mut list = group("起動中の Pi　·　選ぶと会話を開きます", p);
            for (i, v) in rows.group("instance") {
                let id = id.to_string();
                let i = i.clone();
                let nav = n.clone();
                list = list.child(
                    Button::new(SharedString::from(format!("first-{i}")))
                        .justify_start()
                        .label(format!(
                            "{}　·　{}",
                            pi_title(v, &i),
                            v["cwd"].as_str().unwrap_or("")
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.navigate(&nav, Route::Home, cx);
                            this.select(id.clone(), i.clone(), cx);
                        })),
                );
            }
            v = v.child(list);
        }
        v.child(self.nav_button(
            n,
            "first-new",
            "＋ 新しい Pi を起動",
            Route::New(id.to_string()),
            cx,
        ))
    }
}
