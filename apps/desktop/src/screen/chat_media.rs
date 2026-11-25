use super::*;
use image::ImageDecoder;
use pimate_markdown::{Image as MarkdownImage, Table};
use state::{
    Page,
    images::{Source, history_count, history_source},
};
use std::io::Cursor;

#[derive(Clone)]
pub(crate) struct Media {
    pub page: Page,
    pub files: bool,
    pub viewer: Entity<Viewer>,
}
#[derive(Clone)]
pub(crate) struct Gallery {
    page: Page,
    source: GallerySource,
}
#[derive(Clone)]
enum GallerySource {
    History {
        entry: String,
        row: Arc<Value>,
        fetch: bool,
    },
    Markdown {
        entry: String,
        images: Arc<[MarkdownImage]>,
    },
}
impl Gallery {
    pub(crate) fn markdown(page: &Page, entry: &str, images: Arc<[MarkdownImage]>) -> Self {
        Self {
            page: page.clone(),
            source: GallerySource::Markdown {
                entry: entry.into(),
                images,
            },
        }
    }
    fn count(&self) -> u64 {
        match &self.source {
            GallerySource::History { row, .. } => history_count(row),
            GallerySource::Markdown { images, .. } => images.len() as u64,
        }
    }
    fn label(&self, i: u64) -> String {
        if let GallerySource::Markdown { images, .. } = &self.source
            && let Some(v) = images.get(i as usize)
            && !v.alt.is_empty()
        {
            return v.alt.clone();
        }
        format!("画像 {} / {}", i + 1, self.count())
    }
    fn source(&self, index: u64, thumb: bool) -> Option<Source<'_>> {
        match &self.source {
            GallerySource::History { entry, row, fetch } => {
                history_source(entry, row, index, thumb, *fetch)
            }
            GallerySource::Markdown { entry, images } => {
                Source::markdown(images.get(index as usize)?, entry).ok()
            }
        }
    }
}
#[derive(Default)]
pub(crate) struct Viewer {
    pub table: Option<(Table, Gallery)>,
    photo: Option<(Gallery, u64)>,
}
#[derive(Default)]
struct Tile {
    task: Option<Task<()>>,
    image: Option<Result<Arc<RenderImage>, Error>>,
}
fn decode(bytes: &[u8], thumb: bool) -> Result<Arc<RenderImage>, Error> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::InvalidMessage)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(64 << 20);
    reader.limits(limits);
    let mut d = reader.into_decoder().map_err(|_| Error::InvalidMessage)?;
    if d.total_bytes() > 64 << 20 {
        return Err(Error::BadRequest);
    }
    let orientation = d.orientation().map_err(|_| Error::InvalidMessage)?;
    let mut im = image::DynamicImage::from_decoder(d).map_err(|_| Error::InvalidMessage)?;
    im.apply_orientation(orientation);
    let edge = if thumb { 256 } else { 2560 };
    if im.width().max(im.height()) > edge {
        im = im.thumbnail(edge, edge)
    }
    let mut data = im.into_rgba8();
    for p in data.pixels_mut() {
        p.0.swap(0, 2)
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(data)])))
}
fn photo(
    id: SharedString,
    gallery: Gallery,
    index: u64,
    thumb: bool,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let tile = w.use_keyed_state(id, cx, |_, cx| {
        cx.on_release(|t: &mut Tile, cx| {
            if let Some(Ok(im)) = t.image.take() {
                cx.drop_image(im, None)
            }
        })
        .detach();
        Tile::default()
    });
    let content = match &tile.read(cx).image {
        Some(Ok(im)) => img(im.clone())
            .object_fit(ObjectFit::Contain)
            .size_full()
            .into_any_element(),
        Some(Err(_)) => div()
            .text_sm()
            .child("画像を表示できません")
            .into_any_element(),
        None => div().text_sm().child("読み込み中…").into_any_element(),
    };
    div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(content)
        .child(
            canvas(
                move |bounds, w, cx| {
                    let visible = bounds.intersects(&w.content_mask().bounds);
                    tile.update(cx, |t, cx| {
                        if !visible {
                            t.task.take();
                            if let Some(im) = t.image.take() {
                                if let Ok(im) = im {
                                    cx.drop_image(im, Some(w))
                                }
                                cx.notify()
                            }
                            return;
                        }
                        if t.task.is_some() || t.image.is_some() {
                            return;
                        }
                        t.task = Some(cx.spawn(async move |this, cx| {
                            let r = cx
                                .background_executor()
                                .spawn(async move {
                                    let s =
                                        gallery.source(index, thumb).ok_or(Error::Unsupported)?;
                                    let bytes = gallery.page.image(s, thumb).await?;
                                    decode(&bytes, thumb)
                                })
                                .await;
                            let _ = this.update(cx, |t, cx| {
                                t.image = Some(r);
                                cx.notify()
                            });
                        }));
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}
impl Media {
    pub(crate) fn tile(
        &self,
        id: &str,
        gallery: Gallery,
        i: u64,
        w: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let viewer = self.viewer.clone();
        let label = gallery.label(i);
        let f = gallery.clone();
        div()
            .w(rems(9.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(rems(4.5))
                    .child(photo(id.to_owned().into(), gallery, i, true, w, cx)),
            )
            .child(
                Button::new(SharedString::from(format!("open:{id}")))
                    .small()
                    .label(label)
                    .on_click(move |_, _, cx| {
                        viewer.update(cx, |s, cx| {
                            s.photo = Some((f.clone(), i));
                            cx.notify()
                        })
                    }),
            )
            .into_any_element()
    }
    pub(super) fn history(
        &self,
        id: &str,
        v: &Value,
        fetch: bool,
        w: &mut Window,
        cx: &mut App,
    ) -> Div {
        let gallery = Gallery {
            page: self.page.clone(),
            source: GallerySource::History {
                entry: id.into(),
                row: Arc::new(v.clone()),
                fetch,
            },
        };
        let count = gallery.count();
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(
                (0..count.min(4))
                    .map(|i| self.tile(&format!("history:{id}:{i}"), gallery.clone(), i, w, cx)),
            )
            .when(count > 4, |b| b.child(format!("ほか {} 枚", count - 4)))
    }
    pub(super) fn panel(&self, p: &Colors, w: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let v = self.viewer.read(cx);
        let photo_open = v.photo.is_some();
        let viewer = self.viewer.clone();
        let close = Button::new("close-media")
            .label(if photo_open {
                "画像を閉じる"
            } else {
                "表を閉じる"
            })
            .on_click(move |_, _, cx| {
                viewer.update(cx, |s, cx| {
                    if s.photo.take().is_none() {
                        s.table = None
                    }
                    cx.notify()
                })
            });
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(p.border);
        let body = if let Some((g, i)) = v.photo.clone() {
            head = head
                .child(g.label(i))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children([(-1, "前へ"), (1, "次へ")].map(|(dir, label)| {
                            let v = self.viewer.clone();
                            let next = i.checked_add_signed(dir).filter(|&n| n < g.count());
                            Button::new(label)
                                .label(label)
                                .disabled(next.is_none())
                                .on_click(move |_, _, cx| {
                                    v.update(cx, |s, cx| {
                                        if let Some((_, i)) = &mut s.photo
                                            && let Some(next) = next
                                        {
                                            *i = next;
                                            cx.notify()
                                        }
                                    })
                                })
                        })),
                )
                .child(close);
            photo(format!("full:{i}").into(), g, i, false, w, cx)
        } else if let Some((t, g)) = v.table.clone() {
            let text = t.markdown.clone();
            head = head
                .child("表")
                .child(
                    Button::new("copy-table")
                        .label("Markdown をコピー")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                        }),
                )
                .child(close);
            div()
                .id("full-table")
                .size_full()
                .overflow_y_scroll()
                .p_4()
                .child(markdown::table("full", &t, Some((self, &g)), w, cx))
                .into_any_element()
        } else {
            return None;
        };
        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(head)
                .child(div().min_h_0().flex_1().child(body))
                .into_any_element(),
        )
    }
}
impl Desktop {
    pub(super) fn attachments(&self, key: &EditorKey, page: &Page, cx: &Context<Self>) -> Div {
        let rows = &self.machines[&key.host].rows;
        let shown = Arc::new(self.state.attachments.images(key).to_vec());
        let limit = rows
            .get("view", "promptImageLimit")
            .as_u64()
            .unwrap_or(1)
            .min(4) as usize;
        let pg = page.clone();
        let mut row = div().flex().flex_wrap().items_center().gap_1();
        if rows.get("view", "promptImages") == true {
            row = row.child(
                Button::new("pick-images")
                    .small()
                    .label(format!("画像を添付 {}/{limit}", shown.len()))
                    .disabled(shown.len() >= limit || self.state.attachments.busy() || self.sending)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Ok(ticket) = this.start_picker(&pg) else {
                            return;
                        };
                        let paths = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: true,
                            prompt: Some("添付する画像を選択".into()),
                        });
                        let pg = pg.clone();
                        cx.spawn(async move |this, cx| {
                            let result = match paths.await {
                                Ok(Ok(Some(paths))) => cx
                                    .background_executor()
                                    .spawn(
                                        async move { ticket.encode(&paths).map(|v| (ticket, v)) },
                                    )
                                    .await
                                    .map(Some),
                                Ok(Ok(None)) => Ok(None),
                                _ => Err(Error::Unsupported),
                            };
                            let _ = this.update(cx, |this, cx| {
                                if pg.visible(this)
                                    && let Err(e) = result.and_then(|r| {
                                        r.map_or(Ok(()), |(ticket, v)| {
                                            this.finish_picker(ticket, v)
                                        })
                                    })
                                {
                                    this.notice = format!("画像を追加できません: {e}")
                                }
                                cx.notify()
                            });
                        })
                        .detach();
                        cx.notify();
                    })),
            );
        }
        for i in 0..shown.len() {
            let imgs = shown.clone();
            let pg = page.clone();
            row = row.child(
                Button::new(("remove-image", i))
                    .small()
                    .label(format!("画像 {} を外す", i + 1))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if pg.visible(this) {
                            if let Err(e) = this.state.attachments.remove(&pg, &imgs, i) {
                                this.notice = format!("変更できません: {e}")
                            }
                            cx.notify()
                        }
                    })),
            )
        }
        row
    }
}
#[cfg(test)]
mod tests {
    use super::decode;
    #[test]
    fn invalid_image_fails_without_rendering() {
        assert!(decode(b"not an image", false).is_err())
    }
}
