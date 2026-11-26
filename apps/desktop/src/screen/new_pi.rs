use super::*;
use pimate_core::path_complete::{match_folders, split_folder_path};
use pimate_protocol::ModelRef;
use settings::{group, line};
use state::{MachineQuery, SpawnStatus};

pub(super) struct SpawnDraft {
    conn: std::sync::Weak<Connection>,
    folder: String,
    trusted: Option<String>,
    message: String,
    model: Option<ModelRef>,
    thinking: Option<pimate_protocol::ThinkingLevel>,
}
pub(super) struct Fields {
    folder: Entity<InputState>,
    message: Entity<InputState>,
    trusted: Option<String>,
    folders: Value,
    catalog: Value,
    home: String,
    all: bool,
    query: Entity<InputState>,
    _query_watch: Subscription,
    model: Option<ModelRef>,
    expanded: bool,
    thinking: Option<pimate_protocol::ThinkingLevel>,
    ask: bool,
    dirs: Option<(String, Vec<String>)>,
    request: Option<(String, Task<()>)>,
    busy: bool,
    submitted: bool,
    pub(super) submitting: bool,
    _folder_watch: Subscription,
}
fn first_message(w: &mut Window, cx: &mut Context<InputState>) -> InputState {
    // Unlike multiline plain text, AutoGrow leaves Tab / Shift+Tab to the form.
    InputState::new(w, cx)
        .auto_grow(2, 2)
        .placeholder("Pi への依頼")
}
fn spawn_error(error: Error, submitted: &mut bool) -> &'static str {
    if matches!(
        error,
        Error::BadRequest | Error::Rejected | Error::Unsupported | Error::Blocked
    ) {
        *submitted = false;
        "起動要求は拒否されました。フォルダや設定を確認して再実行できます。"
    } else {
        "起動結果を確認できません。再実行の前に一覧を確認してください。"
    }
}
impl Desktop {
    pub(super) fn remember_spawn(&self, n: &Navigation, cx: &mut Context<Self>) {
        n.update(cx, |s, cx| {
            if let Route::New(id) = &s.route
                && let Some(f) = &s.spawn
                && !f.submitted
                && let Some(page) = &s.page
                && page.visible(self)
            {
                s.spawn_drafts.insert(
                    id.clone(),
                    SpawnDraft {
                        conn: Arc::downgrade(&self.machines[id].conn),
                        folder: f.folder.read(cx).value().to_string(),
                        trusted: f.trusted.clone(),
                        message: f.message.read(cx).value().to_string(),
                        model: f.model.clone(),
                        thinking: f.thinking,
                    },
                );
            }
        });
    }
    pub(super) fn new_pi(
        &mut self,
        n: &Navigation,
        p: &Colors,
        id: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if n.read(cx).spawn.is_none() && !self.palette_open {
            self.modal_focus.focus(w);
        }
        let mut machines = line();
        for h in &self.hosts {
            machines = machines.child(
                self.nav_button(
                    n,
                    SharedString::from(format!("spawn-host-{}", h.id)),
                    self.host_label(h),
                    Route::New(h.id.clone()),
                    cx,
                )
                .selected(id == h.id)
                .disabled(n.read(cx).spawn.as_ref().is_some_and(|f| f.submitted)),
            );
        }
        let mut v = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(group("マシン", p).child(machines));
        if !n.read(cx).page.as_ref().is_some_and(|p| p.visible(self)) {
            return v.child("起動先に接続してからマシンを選んでください。");
        };
        if n.read(cx).spawn.is_none() {
            let draft = n
                .update(cx, |s, _| s.spawn_drafts.remove(id))
                .filter(|d| d.conn.ptr_eq(&Arc::downgrade(&self.machines[id].conn)));
            let path = n
                .read(cx)
                .initial_folder
                .clone()
                .or_else(|| draft.as_ref().map(|d| d.folder.clone()))
                .unwrap_or_default();
            let trusted = draft
                .as_ref()
                .and_then(|d| d.trusted.clone())
                .filter(|d| d == &path);
            let folder = cx.new(|cx| {
                InputState::new(w, cx)
                    .placeholder("/path/to/project")
                    .default_value(path)
            });
            folder.focus_handle(cx).focus(w);
            let message = cx.new(|cx| {
                first_message(w, cx).default_value(
                    draft
                        .as_ref()
                        .map(|d| d.message.clone())
                        .unwrap_or_default(),
                )
            });
            let nav = n.downgrade();
            let query = cx.new(|cx| InputState::new(w, cx).placeholder("モデルを検索"));
            let query_watch = cx.subscribe(&query, |_, _, _: &InputEvent, cx| cx.notify());
            let watch = cx.subscribe(&folder, move |_, input, event, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(nav) = nav.upgrade()
                {
                    let path = input.read(cx).value().to_string();
                    nav.update(cx, |s, _| {
                        if let Some(f) = &mut s.spawn
                            && f.folder == input
                        {
                            f.trusted = known_trust(&f.folders, &f.home, &path).then_some(path);
                        }
                    });
                }
                cx.notify();
            });
            n.update(cx, |s, _| {
                s.spawn = Some(Fields {
                    folder,
                    message,
                    trusted,
                    folders: Value::Null,
                    catalog: Value::Null,
                    home: String::new(),
                    all: false,
                    query,
                    _query_watch: query_watch,
                    model: draft.as_ref().and_then(|d| d.model.clone()),
                    expanded: false,
                    thinking: draft.as_ref().and_then(|d| d.thinking),
                    ask: self.state.settings.ask_tool,
                    dirs: None,
                    request: None,
                    busy: true,
                    submitted: false,
                    submitting: false,
                    _folder_watch: watch,
                })
            });
            self.page_task(
                n,
                w,
                cx,
                async move |p| {
                    let folders = p.query(MachineQuery::Folders).await;
                    let models = p.host_models().await;
                    (folders, models)
                },
                |_, n, (folders, models), w, cx| {
                    n.update(cx, |s, cx| {
                        let f = s.spawn.as_mut().unwrap();
                        f.busy = false;
                        if let Ok(mut x) = folders {
                            f.folders = x["folders"].take();
                            f.home = x["home"].as_str().unwrap_or_default().into();
                            if let Some(home) = x["home"]
                                .as_str()
                                .filter(|_| f.folder.read(cx).value().is_empty())
                            {
                                f.folder
                                    .update(cx, |i, cx| i.set_value(home.to_string(), w, cx));
                            }
                        } else {
                            s.note = "最近のフォルダは未確認です。パスを入力してください。".into();
                        }
                        match models {
                            Ok(Some(x)) => f.catalog = x,
                            Err(_) => s
                                .note
                                .push_str(" モデル一覧は未確認です。Pi の既定を利用できます。"),
                            _ => {}
                        }
                    });
                },
            );
        }
        let f = n.read(cx).spawn.as_ref().unwrap();
        let locked = f.submitted;
        let folder = f.folder.clone();
        let msg = f.message.clone();
        let path = folder.read(cx).value().to_string();
        let split = (!locked
            && self.machines[id].rows.get("host", "info")["dirs"] == true
            && folder.focus_handle(cx).is_focused(w))
        .then(|| split_folder_path(&path))
        .flatten();
        if f.request.as_ref().map(|r| r.0.as_str()) != split.map(|r| r.0) {
            let req = split.map(|(parent, _)| {
                let parent = parent.to_owned();
                let shown = parent.clone();
                let page = n.read(cx).page.clone().unwrap();
                let nav = n.downgrade();
                let timer = cx.background_executor().clone();
                (
                    parent,
                    cx.spawn(async move |app, cx| {
                        timer.timer(std::time::Duration::from_millis(150)).await;
                        let result = page.query(MachineQuery::Dirs(shown.clone())).await;
                        let _ = app.update(cx, |app, cx| {
                            if page.visible(app)
                                && let Some(nav) = nav.upgrade()
                            {
                                nav.update(cx, |s, _| {
                                    if let Some(f) = &mut s.spawn
                                        && f.request.as_ref().is_some_and(|r| r.0 == shown)
                                    {
                                        f.dirs = result.ok().and_then(|v| {
                                            Some((
                                                shown,
                                                serde_json::from_value(v["dirs"].clone()).ok()?,
                                            ))
                                        });
                                    }
                                });
                                cx.notify();
                            }
                        });
                    }),
                )
            });
            n.update(cx, |s, _| {
                let f = s.spawn.as_mut().unwrap();
                f.request = req;
                f.dirs = None;
            });
        }
        let f = n.read(cx).spawn.as_ref().unwrap();
        let mut folders = group("フォルダ", p).child(context::field(&folder, locked));
        let mut recent = line();
        for (i, path) in f
            .folders
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["path"].as_str())
            .take(8)
            .enumerate()
        {
            let path = path.to_string();
            let edit = folder.clone();
            recent = recent.child(
                Button::new(("folder", i))
                    .small()
                    .label(path.clone())
                    .disabled(locked)
                    .on_click(move |_, w, cx| {
                        edit.update(cx, |i, cx| i.set_value(path.clone(), w, cx))
                    }),
            );
        }
        if let (Some((base, names)), Some((parent, prefix))) = (&f.dirs, split)
            && base == parent
        {
            for (i, name) in match_folders(names.clone(), prefix).into_iter().enumerate() {
                let next = format!("{parent}{name}/");
                let edit = folder.clone();
                recent = recent.child(
                    Button::new(("dir", i))
                        .small()
                        .label(format!("{name}/"))
                        .on_click(move |_, w, cx| {
                            edit.update(cx, |i, cx| {
                                i.set_value(next.clone(), w, cx);
                                i.focus(w, cx)
                            })
                        }),
                );
            }
        }
        let trusted = f.trusted.as_ref() == Some(&path);
        let nav = n.clone();
        let shown = path.clone();
        folders = folders
            .child(recent)
            .child(
                Button::new("trust")
                    .label(if trusted {
                        "✓ このフォルダを信頼する"
                    } else {
                        "このフォルダを信頼する"
                    })
                    .selected(trusted)
                    .disabled(locked || path.trim().is_empty())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        nav.update(cx, |s, cx| {
                            let f = s.spawn.as_mut().unwrap();
                            if f.folder.read(cx).value().as_ref() == shown {
                                f.trusted = (!trusted).then(|| shown.clone());
                            }
                        });
                        cx.notify();
                    })),
            )
            .child(p.caption("フォルダ内の設定と Extension を読み込みます。オンにすると PC の Pi にも記憶され、次回から確認されません。"));
        let nav = n.clone();
        let mut model = group("モデル", p).child(
            Button::new("pick-model")
                .justify_start()
                .label(format!(
                    "{}　{}",
                    f.model
                        .as_ref()
                        .map(|m| format!("{} · {}", m.id, m.provider))
                        .unwrap_or_else(|| default_label(&f.catalog)),
                    if f.expanded { "▴" } else { "▾" }
                ))
                .disabled(locked)
                .on_click(cx.listener(move |_, _, _, cx| {
                    nav.update(cx, |s, _| {
                        let f = s.spawn.as_mut().unwrap();
                        f.expanded = !f.expanded;
                    });
                    cx.notify();
                })),
        );
        if f.expanded {
            let models = f.catalog["models"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let favorites = f.catalog["scoped"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mut choices = line();
            if !favorites.is_empty() {
                for (all, label) in [(false, "お気に入り"), (true, "すべて")] {
                    let n = n.clone();
                    choices = choices.child(
                        Button::new(label)
                            .small()
                            .label(label)
                            .selected(f.all == all)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                n.update(cx, |s, _| s.spawn.as_mut().unwrap().all = all);
                                cx.notify()
                            })),
                    );
                }
            }
            let term = if models.len() > 6 {
                choices = choices.child(context::field(&f.query, locked));
                f.query.read(cx).value().trim().to_lowercase()
            } else {
                String::new()
            };
            let entries = models.iter().filter(|m| {
                (f.all
                    || favorites.is_empty()
                    || favorites
                        .iter()
                        .any(|v| v["provider"] == m["provider"] && v["id"] == m["id"]))
                    && format!(
                        "{}/{} {}",
                        m["provider"].as_str().unwrap_or(""),
                        m["id"].as_str().unwrap_or(""),
                        m["name"].as_str().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&term)
            });
            let entries = entries
                .filter_map(|v| {
                    Some(Some(ModelRef {
                        provider: v["provider"].as_str()?.into(),
                        id: v["id"].as_str()?.into(),
                    }))
                })
                .collect::<Vec<_>>();
            if entries.is_empty() && !f.busy {
                choices = choices.child(p.caption(if f.catalog.is_null() {
                    "モデル一覧を取得できません。"
                } else {
                    "一致するモデルはありません"
                }));
            }
            for (i, m) in term
                .is_empty()
                .then_some(None)
                .into_iter()
                .chain(entries)
                .enumerate()
            {
                let nav = n.clone();
                choices = choices.child(
                    Button::new(("model", i))
                        .small()
                        .selected(f.model == m)
                        .disabled(locked)
                        .label(
                            m.as_ref()
                                .map(|m| format!("{} · {}", m.id, m.provider))
                                .unwrap_or_else(|| default_label(&f.catalog)),
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            nav.update(cx, |s, _| {
                                let f = s.spawn.as_mut().unwrap();
                                f.model = m.clone();
                                f.thinking = None;
                                f.expanded = false;
                            });
                            cx.notify();
                        })),
                );
            }
            model = model.child(choices);
        }
        if let Some(selected) = &f.model
            && let Some(levels) = f.catalog["models"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|m| m["id"] == selected.id && m["provider"] == selected.provider)
                .and_then(|m| m["thinkingLevels"].as_array())
                .filter(|l| l.len() > 1)
        {
            let mut choices = line().child(p.caption("Thinking"));
            for (i, (name, level)) in std::iter::once(("既定", None))
                .chain(levels.iter().filter_map(|l| {
                    Some((l.as_str()?, Some(serde_json::from_value(l.clone()).ok()?)))
                }))
                .enumerate()
            {
                let nav = n.clone();
                choices = choices.child(
                    Button::new(("thinking", i))
                        .small()
                        .label(name.to_string())
                        .selected(f.thinking == level)
                        .disabled(locked)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            nav.update(cx, |s, _| s.spawn.as_mut().unwrap().thinking = level);
                            cx.notify();
                        })),
                );
            }
            model = model.child(choices);
        }
        let ask = f.ask;
        let nav = n.clone();
        model = model.child(
            Button::new("spawn-ask")
                .small()
                .ghost()
                .justify_start()
                .label(if ask {
                    "✓ 質問ツール（pimate_ask）　オン"
                } else {
                    "質問ツール（pimate_ask）　オフ"
                })
                .selected(ask)
                .disabled(locked)
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.state.settings.ask_tool = !ask;
                    let failed = app
                        .settings_path
                        .as_ref()
                        .is_none_or(|path| app.state.settings.save(path).is_err());
                    nav.update(cx, |s, _| {
                        s.pref_error = failed;
                        s.spawn.as_mut().unwrap().ask = !ask
                    });
                    cx.notify();
                })),
        );
        v = v.child(folders).child(model);
        if f.busy {
            v = v.child(p.caption("候補を取得中…"));
        }
        v.child(
            group("最初のメッセージ（任意）", p).child(
                context::editor(div(), msg.clone(), locked)
                    .child(Input::new(&msg).h(rems(3.5)).disabled(locked)),
            ),
        )
    }
    pub(super) fn spawn_footer(
        &self,
        n: &Navigation,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Option<Div> {
        let f = n.read(cx).spawn.as_ref()?;
        let page = n.read(cx).page.clone().filter(|p| p.visible(self))?;
        let id = page.host().to_string();
        let disabled = f.submitted
            || f.folder.read(cx).value().trim().is_empty()
            || self.machines.get(&id)?.rows.get("host", "connection") != "connected";
        let nav = n.clone();
        let submit = cx.listener(move |app, _, w, cx| {
            let f = nav.read(cx).spawn.as_ref().unwrap();
            if f.submitted || !page.visible(app) {
                return;
            }
            let path = f.folder.read(cx).value().to_string();
            let mut form = match page.spawn_form(f.ask) {
                Ok(f) => f,
                Err(_) => return,
            };
            form.folder(path.clone(), f.trusted.as_ref() == Some(&path));
            form.message = f.message.read(cx).value().to_string();
            form.model = f.model.clone();
            form.thinking = f.thinking;
            if [f.folder.clone(), f.message.clone()]
                .iter()
                .any(|i| i.update(cx, |i, cx| i.marked_text_range(w, cx).is_some()))
            {
                return;
            }
            nav.update(cx, |s, _| {
                let f = s.spawn.as_mut().unwrap();
                f.submitting = true;
                f.submitted = true;
                s.note = "起動要求の結果を確認しています…".into();
            });
            let timer = cx.background_executor().clone();
            let host = id.clone();
            app.page_task(
                &nav,
                w,
                cx,
                async move |_| {
                    let x = form.spawn().await?;
                    loop {
                        let status = x.status().map_err(|_| Error::OutcomeUnknown)?;
                        if status != SpawnStatus::Waiting {
                            break Ok::<_, Error>((x.id, status));
                        }
                        timer.timer(std::time::Duration::from_millis(200)).await;
                    }
                },
                move |app, n, res, _, cx| {
                    n.update(cx, |s, _| s.spawn.as_mut().unwrap().submitting = false);
                    let msg = match res {
                        Ok((id, SpawnStatus::Ready)) => {
                            app.navigate(n, Route::Home, cx);
                            app.select(host, id, cx);
                            return;
                        }
                        Ok((_, SpawnStatus::MessageUnconfirmed)) => {
                            "Pi は起動しましたが、最初のメッセージは未確認です。"
                        }
                        Ok(_) => "起動を受け付けましたが、一覧にはまだ表示されていません。",
                        Err(e) => n.update(cx, |s, _| {
                            spawn_error(e, &mut s.spawn.as_mut().unwrap().submitted)
                        }),
                    };
                    n.update(cx, |s, _| s.note = msg.into());
                },
            );
        });
        Some(
            line()
                .flex_shrink_0()
                .justify_between()
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(p.border)
                .child(p.caption("起動した Pi はマシン一覧に表示されます。"))
                .child(
                    p.primary("spawn", cx)
                        .label("起動")
                        .disabled(disabled)
                        .on_click(submit),
                ),
        )
    }
}
fn default_label(c: &Value) -> String {
    let saved = &c["default"];
    let m = &saved["model"];
    let Some(id) = m["id"].as_str().filter(|id| !id.is_empty()) else {
        return "Pi の既定".into();
    };
    let entry = c["models"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["id"] == m["id"] && v["provider"] == m["provider"]);
    let name = entry
        .and_then(|v| v["name"].as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or(id);
    let thinking = saved["thinking"].as_str().filter(|v| {
        !v.is_empty()
            && *v != "off"
            && entry
                .and_then(|e| e["thinkingLevels"].as_array())
                .is_none_or(|v| v.len() != 1)
    });
    format!(
        "Pi の既定（{name}{}）",
        thinking.map_or(String::new(), |t| format!(" · {t}"))
    )
}
fn known_trust(rows: &Value, home: &str, path: &str) -> bool {
    let path = if !home.is_empty() && (path == "~" || path.starts_with("~/")) {
        format!("{}{}", home, &path[1..])
    } else {
        path.to_string()
    };
    fn strip(path: &str) -> &str {
        if path.starts_with('/') && path.trim_end_matches('/').is_empty() {
            "/"
        } else {
            path.trim_end_matches('/')
        }
    }
    rows.as_array().into_iter().flatten().any(|v| {
        !path.is_empty()
            && v["path"]
                .as_str()
                .is_some_and(|v| !v.is_empty() && strip(v) == strip(&path))
            && v["trusted"] == true
    })
}
#[cfg(test)]
mod tests {
    use super::known_trust;
    #[test]
    fn default_model_and_scoped_path_completion() {
        use super::*;
        let mut c = serde_json::json!({"default":{"model":{"provider":"a","id":"m"},"thinking":"high"},"models":[{"id":"m","provider":"b","name":"other"},{"id":"m","provider":"a","name":"選択","thinkingLevels":["off","high"]}]});
        assert_eq!(default_label(&c), "Pi の既定（選択 · high）");
        c["models"][1]["thinkingLevels"] = serde_json::json!(["off"]);
        assert_eq!(default_label(&c), "Pi の既定（選択）");
        assert_eq!(default_label(&Value::Null), "Pi の既定");
        let (base, prefix) = split_folder_path("~/code/s").unwrap();
        let v = match_folders(vec!["src".into(), "other".into()], prefix);
        assert_eq!(format!("{base}{}/", v[0]), "~/code/src/");
        assert!(known_trust(
            &serde_json::json!([{"path":"/home/p/code","trusted":true}]),
            "/home/p",
            "~/code/"
        ));
    }
    #[gpui::test]
    fn palette_escape_returns_to_message_field(cx: &mut gpui::TestAppContext) {
        use gpui::Focusable;
        let (app, w, _) = super::super::tests::open_chat(cx);
        w.update(|_, cx| super::super::keys::init(cx));
        w.simulate_keystrokes("secondary-n");
        let field = app.read_with(w, |d, cx| {
            d.navigation
                .as_ref()
                .unwrap()
                .read(cx)
                .spawn
                .as_ref()
                .unwrap()
                .message
                .clone()
        });
        w.update(|w, cx| field.focus_handle(cx).focus(w));
        w.simulate_keystrokes("a secondary-k escape b");
        assert!(w.update(|w, cx| field.focus_handle(cx).is_focused(w)));
        assert_eq!(field.read_with(w, |s, _| s.value().to_string()), "ab");
    }
    #[gpui::test]
    fn escape_retains_unsent_request_for_same_machine(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = super::super::tests::open_chat(cx);
        w.update(|_, cx| super::super::keys::init(cx));
        w.simulate_keystrokes("secondary-n");
        w.update(|w, cx| {
            let n = app.read(cx).navigation.clone().unwrap();
            let f = n.read(cx).spawn.as_ref().unwrap();
            let (folder, msg) = (f.folder.clone(), f.message.clone());
            folder.update(cx, |s, cx| s.set_value("/tmp/project", w, cx));
            msg.update(cx, |s, cx| {
                s.set_value("入力途中", w, cx);
                s.focus(w, cx);
            });
        });
        w.simulate_keystrokes("escape");
        assert!(
            app.read_with(w, |d, cx| d.navigation.as_ref().unwrap().read(cx).route
                == super::Route::Home)
        );
        w.simulate_keystrokes("secondary-n");
        app.read_with(w, |d, cx| {
            let f = d
                .navigation
                .as_ref()
                .unwrap()
                .read(cx)
                .spawn
                .as_ref()
                .unwrap();
            assert_eq!(f.folder.read(cx).value().as_str(), "/tmp/project");
            assert_eq!(f.message.read(cx).value().as_str(), "入力途中");
        });
    }
    #[test]
    fn only_definite_refusal_unlocks_spawn() {
        use pimate_core::Error;
        for e in [
            Error::Rejected,
            Error::BadRequest,
            Error::Blocked,
            Error::Unsupported,
        ] {
            let mut locked = true;
            assert!(super::spawn_error(e, &mut locked).contains("拒否"));
            assert!(!locked);
        }
        for e in [
            Error::OutcomeUnknown,
            Error::Timeout,
            Error::Offline,
            Error::InvalidMessage,
            Error::StaleTarget,
        ] {
            let mut locked = true;
            assert!(super::spawn_error(e, &mut locked).contains("一覧を確認"));
            assert!(locked);
        }
    }
    #[gpui::test]
    fn multiline_message_tabs_to_spawn_without_changing_the_request(cx: &mut gpui::TestAppContext) {
        use gpui::{Context, Entity, Focusable, KeyUpEvent, Keystroke, Window, div, prelude::*};
        use gpui_component::{
            Root,
            button::Button,
            input::{Input, InputState},
        };
        use std::{cell::Cell, rc::Rc};
        struct Form {
            message: Entity<InputState>,
            clicked: Rc<Cell<&'static str>>,
        }
        impl Render for Form {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let key = |id| {
                    let clicked = self.clicked.clone();
                    Button::new(id)
                        .label(id)
                        .on_click(move |_, _, _| clicked.set(id))
                };
                div()
                    .flex()
                    .flex_col()
                    .child(key("previous"))
                    .child(Input::new(&self.message))
                    .child(key("spawn"))
            }
        }
        cx.update(gpui_component::init);
        let clicked = Rc::new(Cell::new(""));
        let mut input = None;
        let (_, w) = cx.add_window_view(|w, cx| {
            let message = cx.new(|cx| super::first_message(w, cx));
            input = Some(message.clone());
            let form = cx.new(|_| Form {
                message,
                clicked: clicked.clone(),
            });
            Root::new(form, w, cx)
        });
        let input = input.unwrap();
        w.update(|w, cx| w.focus(&input.focus_handle(cx)));
        w.simulate_keystrokes("a enter b tab");
        w.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(clicked.get(), "spawn");
        w.update(|w, cx| w.focus(&input.focus_handle(cx)));
        w.simulate_keystrokes("shift-tab");
        w.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(clicked.get(), "previous");
        assert_eq!(w.update(|_, cx| input.read(cx).value().to_string()), "a\nb");
    }
    #[test]
    fn trusted_path_is_exact() {
        let r = serde_json::json!([{"path":"/a ","trusted":true}]);
        assert!(known_trust(&r, "", "/a "));
        assert!(!known_trust(&r, "", "/a"));
    }
}
