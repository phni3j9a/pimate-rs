use super::*;
use pimate_protocol::UIQuestionAnswer;
use state::Page;

pub(super) struct Form {
    values: Vec<Entity<InputState>>,
    selected: Vec<Vec<String>>,
    busy: bool,
    shown: Arc<Value>,
    life: std::sync::Weak<()>,
}
impl Form {
    fn new(life: &Arc<()>, q: &Value, w: &mut Window, cx: &mut App) -> Self {
        let count = q["questions"].as_array().map_or(1, Vec::len);
        Self {
            values: (0..count)
                .map(|_| {
                    cx.new(|cx| {
                        InputState::new(w, cx)
                            .multi_line(q["kind"] == "editor")
                            .default_value(if q["kind"] == "editor" {
                                q["message"].as_str().unwrap_or("").to_owned()
                            } else {
                                String::new()
                            })
                            .placeholder(if q["kind"] == "questionnaire" {
                                "その他（自由入力）"
                            } else {
                                "回答を入力"
                            })
                    })
                })
                .collect(),
            selected: vec![vec![]; count],
            busy: false,
            life: Arc::downgrade(life),
            shown: Arc::new(q.clone()),
        }
    }
}
// Keep the editor mounted for the pending question, not for the card's visibility.
pub(super) fn pending_form(
    key: &EditorKey,
    life: &Arc<()>,
    id: &str,
    q: &Value,
    w: &mut Window,
    cx: &mut App,
) -> Entity<Form> {
    let form = w.use_keyed_state(
        SharedString::from(format!("question:{key:?}:{id}")),
        cx,
        |w, cx| Form::new(life, q, w, cx),
    );
    if form.read(cx).shown.as_ref() != q || !form.read(cx).life.ptr_eq(&Arc::downgrade(life)) {
        form.update(cx, |f, cx| *f = Form::new(life, q, w, cx));
    }
    form
}
fn questionnaire_fields(kind: &str, count: usize, part: &Value, title: &Value) -> (bool, bool) {
    let custom = kind == "questionnaire";
    (custom && (count > 1 || part["question"] != *title), custom)
}
fn choose(values: &mut Vec<String>, multiple: bool, value: String) {
    if multiple && values.contains(&value) {
        values.retain(|v| v != &value)
    } else {
        if !multiple {
            values.clear()
        }
        values.push(value)
    }
}
impl Desktop {
    pub(super) fn question_card(
        &self,
        page: &Page,
        id: &str,
        form: Entity<Form>,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = form.read(cx).shown.clone();
        let q = shown.as_ref();
        let mut card = div()
            .id("question-card")
            .debug_selector(|| "question-card".into())
            .max_h(rems(18.))
            .overflow_y_scroll()
            .mx_4()
            .py_3()
            .px_4()
            .bg(p.surface)
            .rounded_lg()
            .child(p.caption("Pi からの質問").text_color(p.waiting))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(q["title"].as_str().unwrap_or("入力待ち").to_owned()),
            );
        let kind = q["kind"].as_str().unwrap_or("");
        if kind != "editor"
            && let Some(text) = q["message"].as_str()
        {
            card = card.child(text.to_owned())
        }
        let mut valid = true;
        match kind {
            "select" | "questionnaire" => {
                let parts = q["questions"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(std::slice::from_ref(q));
                for (i, part) in parts.iter().enumerate() {
                    let multiple = part["multiSelect"] == true;
                    let (prompt, custom) =
                        questionnaire_fields(kind, parts.len(), part, &q["title"]);
                    let field = form.read(cx).values[i].clone();
                    let selected = &form.read(cx).selected[i];
                    let text = field.read(cx).value();
                    if !multiple && selected.is_empty() && text.trim().is_empty() {
                        valid = false
                    }
                    if prompt {
                        card = card
                            .child(p.caption(part["question"].as_str().unwrap_or("").to_owned()));
                    }
                    for (j, o) in part["options"].as_array().into_iter().flatten().enumerate() {
                        let name = o.as_str().or(o["label"].as_str()).unwrap_or("").to_owned();
                        let selected =
                            selected.contains(&name) && (!custom || multiple || text.is_empty());
                        let f = form.clone();
                        let field = field.clone();
                        let desc = o["description"].as_str().unwrap_or("");
                        card = card.child(
                            div()
                                .py_1()
                                .child(
                                    Button::new(SharedString::from(format!("answer:{i}:{j}")))
                                        .small()
                                        .label(format!(
                                            "{} {name}",
                                            if selected { "✓" } else { "○" }
                                        ))
                                        .disabled(form.read(cx).busy)
                                        .on_click(move |_, w, cx| {
                                            if f.read(cx).busy
                                                || f.read(cx).values.get(i) != Some(&field)
                                            {
                                                return;
                                            }
                                            if !multiple {
                                                field.update(cx, |s, cx| s.set_value("", w, cx));
                                            }
                                            f.update(cx, |f, cx| {
                                                choose(&mut f.selected[i], multiple, name.clone());
                                                cx.notify()
                                            })
                                        }),
                                )
                                .when(!desc.is_empty(), |b| b.child(p.caption(desc.to_owned()))),
                        );
                    }
                    if custom {
                        card = card.child(Input::new(&field).disabled(form.read(cx).busy));
                    }
                }
            }
            "input" | "editor" => {
                card = card.child(Input::new(&form.read(cx).values[0]).disabled(form.read(cx).busy))
            }
            "confirm" => {}
            _ => {
                return card
                    .child("Pi を起動した PC で回答してください。")
                    .into_any_element();
            }
        }
        let mut actions = div().mt_2().flex().gap_2();
        for yes in [false, true] {
            let f = form.clone();
            let pg = page.clone();
            let id = id.to_owned();
            let shown = shown.clone();
            let kind = kind.to_owned();
            actions = actions.child(
                (if yes {
                    p.primary("answer", cx)
                } else {
                    Button::new("cancel-answer")
                })
                .label(match (yes, kind.as_str()) {
                    (true, "confirm") => "はい",
                    (false, "confirm") => "いいえ",
                    (true, _) => "回答する",
                    _ => "キャンセル",
                })
                .disabled(form.read(cx).busy || (yes && !valid))
                .on_click(cx.listener(move |this, _, w, cx| {
                    if !pg.visible(this)
                        || f.read(cx).busy
                        || !Arc::ptr_eq(&f.read(cx).shown, &shown)
                    {
                        return;
                    }
                    let form = f.read(cx);
                    let Ok(mut answer) = pg.question(&id, &shown) else {
                        return;
                    };
                    if kind == "confirm" {
                        answer.fields.confirmed = Some(yes)
                    } else if !yes {
                        answer.fields.cancelled = Some(true)
                    } else {
                        let values = form.values.clone();
                        let selected = form.selected.clone();
                        let mut texts = vec![];
                        for v in values {
                            let text = v.read(cx).value().to_string();
                            if !committed_input_matches(&v, &text, w, cx) {
                                return;
                            }
                            texts.push(text)
                        }
                        match kind.as_str() {
                            "select" => answer.fields.value = selected[0].first().cloned(),
                            "questionnaire" => {
                                answer.fields.answers = Some(
                                    selected
                                        .into_iter()
                                        .zip(texts)
                                        .enumerate()
                                        .map(|(i, (mut selected, t))| {
                                            let custom = (!t.trim().is_empty()).then_some(t);
                                            if custom.is_some()
                                                && shown["questions"][i]["multiSelect"] != true
                                            {
                                                selected.clear()
                                            }
                                            UIQuestionAnswer { selected, custom }
                                        })
                                        .collect(),
                                )
                            }
                            _ => answer.fields.value = texts.pop(),
                        }
                    }
                    f.update(cx, |f, cx| {
                        f.busy = true;
                        cx.notify()
                    });
                    let f = f.clone();
                    let pg = pg.clone();
                    let shown = shown.clone();
                    cx.spawn(async move |this, cx| {
                        let r = answer.answer().await;
                        let _ = this.update(cx, |this, cx| {
                            f.update(cx, |f, cx| {
                                if Arc::ptr_eq(&f.shown, &shown) {
                                    f.busy = false;
                                    cx.notify()
                                }
                            });
                            if pg.visible(this) {
                                this.finish_send_notice(r.map(|_| String::new()));
                                cx.notify()
                            }
                        });
                    })
                    .detach();
                })),
            );
        }
        // Input edits must also refresh the completeness of a questionnaire.
        let inputs = form.read(cx).values.clone();
        let _ = w.use_keyed_state(
            SharedString::from(format!(
                "observe:{}",
                inputs.first().map_or(0, |e| e.entity_id().as_u64())
            )),
            cx,
            |_, cx| {
                inputs
                    .iter()
                    .map(|e| cx.observe(e, |_, _, cx| cx.notify()))
                    .collect::<Vec<_>>()
            },
        );
        card.child(actions).into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::{choose, questionnaire_fields};
    #[test]
    fn duplicate_prompt_only_hides_its_label() {
        let q = serde_json::json!({"question":"Topic"});
        assert_eq!(
            questionnaire_fields("questionnaire", 1, &q, &q["question"]),
            (false, true)
        );
        assert_eq!(
            questionnaire_fields("questionnaire", 2, &q, &q["question"]),
            (true, true)
        );
        assert_eq!(
            questionnaire_fields("select", 1, &q, &q["question"]),
            (false, false)
        );
    }
    #[gpui::test]
    fn panel_roundtrip_retains_only_the_same_pending_answer(cx: &mut gpui::TestAppContext) {
        use super::{EditorKey, Form, pending_form};
        use gpui::{Context, Entity, Window, div, prelude::*};
        use gpui_component::input::Input;
        use std::sync::Arc;
        struct Chat {
            key: EditorKey,
            q: serde_json::Value,
            life: Arc<()>,
            panel: bool,
            form: Option<Entity<Form>>,
        }
        impl Render for Chat {
            fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let f = pending_form(&self.key, &self.life, "q", &self.q, w, cx);
                self.form = Some(f.clone());
                div().when(!self.panel, |d| d.child(Input::new(&f.read(cx).values[0])))
            }
        }
        cx.update(gpui_component::init);
        let (chat, w) = cx.add_window_view(|_, _| Chat {
            key: EditorKey {
                host: "a".into(),
                instance: "pi".into(),
                session: Some("s".into()),
            },
            q: serde_json::json!({"kind":"questionnaire","questions":[{}]}),
            life: Arc::new(()),
            panel: false,
            form: None,
        });
        for changed in ["panel", "question", "lifetime", "destination"] {
            let form = chat.read_with(w, |c, _| c.form.clone().unwrap());
            let rendered = form.read_with(w, |f, _| f.shown.clone());
            w.update(|w, cx| {
                form.update(cx, |f, cx| {
                    f.values[0].update(cx, |v, cx| v.set_value("draft", w, cx));
                    f.selected[0] = vec!["A".into()]
                })
            });
            chat.update(w, |c, cx| {
                c.panel = true;
                cx.notify()
            });
            w.run_until_parked();
            chat.update(w, |c, cx| {
                match changed {
                    "question" => c.q["title"] = "new".into(),
                    "lifetime" => c.life = Arc::new(()),
                    "destination" => c.key.host = "b".into(),
                    _ => {}
                }
                c.panel = false;
                cx.notify()
            });
            w.run_until_parked();
            chat.read_with(w, |c, cx| {
                let f = c.form.as_ref().unwrap().read(cx);
                assert_eq!(
                    f.values[0].read(cx).value().as_str(),
                    if changed == "panel" { "draft" } else { "" }
                );
                assert_eq!(f.selected[0].contains(&"A".into()), changed == "panel");
                assert_eq!(Arc::ptr_eq(&rendered, &f.shown), changed == "panel");
            });
        }
    }
    #[test]
    fn choices_start_empty_and_toggle_without_duplicates() {
        let mut a = vec![];
        choose(&mut a, true, "A".into());
        choose(&mut a, true, "B".into());
        choose(&mut a, true, "A".into());
        assert_eq!(a, ["B"]);
        choose(&mut a, false, "C".into());
        assert_eq!(a, ["C"]);
    }
}
