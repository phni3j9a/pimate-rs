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
    pub(crate) fn http_link(&self, index: u64, id: String) -> Option<Button> {
        let GallerySource::Markdown { images, .. } = &self.source else {
            return None;
        };
        let image = images.get(index as usize)?;
        let url = &image.src;
        if !url
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
        {
            return None;
        }
        let url = url.clone();
        Some(
            Button::new(SharedString::from(id))
                .small()
                .label(format!(
                    "リンク: {}",
                    if image.alt.is_empty() {
                        &url
                    } else {
                        image.alt.as_str()
                    }
                ))
                .on_click(move |_, _, cx| cx.open_url(&url)),
        )
    }
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
    pub table: Option<(Table, Option<Gallery>)>,
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
#[derive(Default)]
struct Zoom {
    scale: Option<f32>,
    offset: Point<Pixels>,
    drag: Option<Point<Pixels>>,
}
impl Zoom {
    fn geometry(
        &self,
        view: Size<Pixels>,
        native: Size<Pixels>,
    ) -> (f32, Point<Pixels>, Size<Pixels>) {
        let scale = self.scale.unwrap_or_else(|| {
            (view.width / native.width)
                .min(view.height / native.height)
                .min(1.)
        });
        let scaled = size(native.width * scale, native.height * scale);
        let axis = |available: Pixels, length: Pixels, offset: Pixels| {
            let center = (available - length) * 0.5;
            if center >= px(0.) {
                center
            } else {
                (center + offset).clamp(center * 2., px(0.))
            }
        };
        (
            scale,
            point(
                axis(view.width, scaled.width, self.offset.x),
                axis(view.height, scaled.height, self.offset.y),
            ),
            scaled,
        )
    }
}
fn zoomed(id: SharedString, im: Arc<RenderImage>, w: &mut Window, cx: &mut App) -> AnyElement {
    let state = w.use_keyed_state(SharedString::from(format!("zoom:{id}")), cx, |_, _| {
        Zoom::default()
    });
    let viewport = std::rc::Rc::new(std::cell::Cell::new(Size::<Pixels>::default()));
    let dimensions = viewport.clone();
    let native = im
        .size(0)
        .map(|d| px(u32::from(d) as f32 / w.scale_factor()));
    let view = state.clone();
    let scroll = state.clone();
    let press = state.clone();
    let drag = state.clone();
    let area = viewport.clone();
    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .on_scroll_wheel(move |e, _, cx| {
            let dy = f32::from(e.delta.pixel_delta(px(36.)).y);
            scroll.update(cx, |z, cx| {
                let (old, _, _) = z.geometry(viewport.get(), native);
                z.scale = Some((old * (dy / 240.).exp()).clamp(0.02, 16.));
                cx.notify()
            });
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Left, move |e, _, cx| {
            press.update(cx, |z, _| z.drag = Some(e.position))
        })
        .on_mouse_move(move |e, _, cx| {
            drag.update(cx, |z, cx| {
                if e.pressed_button != Some(MouseButton::Left) {
                    z.drag = None;
                    return;
                }
                if let Some(previous) = z.drag {
                    z.drag = Some(e.position);
                    let (_, origin, scaled) = z.geometry(area.get(), native);
                    let center = point(
                        (area.get().width - scaled.width) * 0.5,
                        (area.get().height - scaled.height) * 0.5,
                    );
                    z.offset = origin - center + (e.position - previous);
                    cx.notify();
                }
            })
        })
        .child(
            canvas(
                move |bounds, _, _| dimensions.set(bounds.size),
                move |bounds, _, w, cx| {
                    let (_, origin, size) = view.read(cx).geometry(bounds.size, native);
                    let _ = w.paint_image(
                        Bounds::new(bounds.origin + origin, size),
                        Default::default(),
                        im,
                        0,
                        false,
                    );
                },
            )
            .size_full(),
        )
        .child(
            div().absolute().right_2().bottom_2().child(
                Button::new("reset-zoom")
                    .label("等倍に戻す")
                    .on_click(move |_, _, cx| {
                        state.update(cx, |z, cx| {
                            *z = Zoom {
                                scale: Some(1.),
                                ..Zoom::default()
                            };
                            cx.notify()
                        })
                    }),
            ),
        )
        .into_any_element()
}
fn photo(
    id: SharedString,
    gallery: Gallery,
    index: u64,
    thumb: bool,
    w: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let tile = w.use_keyed_state(id.clone(), cx, |_, cx| {
        cx.on_release(|t: &mut Tile, cx| {
            if let Some(Ok(im)) = t.image.take() {
                cx.drop_image(im, None)
            }
        })
        .detach();
        Tile::default()
    });
    let content = match &tile.read(cx).image {
        Some(Ok(im)) if !thumb => zoomed(id, im.clone(), w, cx),
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
        Viewer::panel(&self.viewer, Some(self), p, w, cx)
    }
}
impl Viewer {
    pub(super) fn close(&mut self) {
        if self.photo.take().is_none() {
            self.table = None
        }
    }
    pub(super) fn panel(
        viewer: &Entity<Self>,
        media: Option<&Media>,
        p: &Colors,
        w: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let v = viewer.read(cx);
        let photo_open = v.photo.is_some();
        let dismiss = viewer.clone();
        let close = Button::new("close-media")
            .debug_selector(|| "close-media".into())
            .label(if photo_open {
                "画像を閉じる"
            } else {
                "表を閉じる"
            })
            .on_click(move |_, _, cx| {
                dismiss.update(cx, |s, cx| {
                    s.close();
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
        let body = if let Some((g, i)) = v.photo.clone().filter(|_| media.is_some()) {
            head = head
                .child(g.label(i))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children([(-1, "前へ"), (1, "次へ")].map(|(dir, label)| {
                            let v = viewer.clone();
                            let next = i.checked_add_signed(dir).filter(|&n| n < g.count());
                            Button::new(label)
                                .debug_selector(|| label.to_owned())
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
            if let Some(link) = g.http_link(i, "full-link".into()) {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(link.debug_selector(|| "gallery-http".into()))
                    .into_any_element()
            } else {
                photo(format!("full:{i}").into(), g, i, false, w, cx)
            }
        } else if let Some((t, g)) = v.table.clone() {
            let text = t.markdown.clone();
            head = head
                .child("表")
                .child(
                    Button::new("copy-table")
                        .debug_selector(|| "copy-table".into())
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
                .child(markdown::table("full", &t, media.zip(g.as_ref()), w, cx))
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
pub(super) enum ImageInput {
    Files(Vec<std::path::PathBuf>),
    Clipboard(ClipboardImageReader),
}
impl Desktop {
    pub(super) fn add_images(
        &mut self,
        page: &Page,
        source: ImageInput,
        w: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay_open(w, cx) || !page.visible(self) {
            return;
        }
        let ticket = match self.start_picker(page) {
            Ok(t) => t,
            Err(e) => {
                self.notice = format!("画像を追加できません: {e}");
                cx.notify();
                return;
            }
        };
        let page = page.clone();
        cx.spawn(async move |app, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let encoded = match source {
                        ImageInput::Files(paths) => ticket.encode(&paths),
                        ImageInput::Clipboard(read) => read(state::images::SOURCE as usize)
                            .map_err(|_| Error::BadRequest)
                            .and_then(|bytes| ticket.clipboard(&[bytes])),
                    };
                    encoded.map(|v| (ticket, v))
                })
                .await;
            let _ = app.update(cx, |app, cx| {
                if page.visible(app)
                    && let Err(e) = result.and_then(|(t, v)| app.finish_picker(t, v))
                {
                    app.notice = format!("画像を追加できません: {e}");
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn attachments(
        &self,
        key: &EditorKey,
        page: &Page,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
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
        if rows.get("view", "promptImages") != true {
            row = row.child(
                Button::new("unsupported-images")
                    .ghost()
                    .small()
                    .label("画像添付不可")
                    .tooltip(
                        "この Pi の RC は画像添付に対応していません。PC で拡張を更新してください。",
                    ),
            );
        }
        for i in 0..shown.len() {
            let imgs = shown.clone();
            let pg = page.clone();
            let asset =
                w.use_keyed_state(("draft-photo", i), cx, |_, _| None::<(String, Arc<Image>)>);
            if asset
                .read(cx)
                .as_ref()
                .is_none_or(|(s, _)| s != &imgs[i].data)
            {
                use base64::Engine;
                let im = base64::engine::general_purpose::STANDARD
                    .decode(&imgs[i].data)
                    .ok()
                    .and_then(|data| {
                        // Restored drafts can originate on another client. Reject unbounded dimensions before GPUI decodes them.
                        let reader = image::ImageReader::new(Cursor::new(&data))
                            .with_guessed_format()
                            .ok()?;
                        let format = match reader.format()? {
                            image::ImageFormat::Jpeg => ImageFormat::Jpeg,
                            image::ImageFormat::Png => ImageFormat::Png,
                            image::ImageFormat::WebP => ImageFormat::Webp,
                            _ => return None,
                        };
                        let (w, h) = reader.into_dimensions().ok()?;
                        if w.max(h) > 4096 || u64::from(w) * u64::from(h) > 4_000_000 {
                            return None;
                        }
                        Some(Arc::new(Image::from_bytes(format, data)))
                    });
                asset.update(cx, |s, _| *s = im.map(|im| (imgs[i].data.clone(), im)));
            }
            let preview = asset.read(cx).as_ref().map(|(_, im)| {
                img(im.clone())
                    .size(rems(2.))
                    .object_fit(ObjectFit::Contain)
            });
            row = row.child(
                div().flex().items_center().children(preview).child(
                    Button::new(("remove-image", i))
                        .debug_selector(move || format!("remove-image-{i}"))
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
                ),
            )
        }
        row
    }
}
#[cfg(test)]
mod tests {
    use super::decode;
    #[test]
    fn fitted_zoom_and_pan_stay_inside_the_image() {
        use super::*;
        let view = size(px(200.), px(120.));
        let native = size(px(400.), px(100.));
        assert_eq!(
            Zoom::default().geometry(view, native),
            (0.5, point(px(0.), px(35.)), size(px(200.), px(50.)))
        );
        let mut z = Zoom {
            scale: Some(1.),
            offset: point(px(45.), px(0.)),
            drag: None,
        };
        assert_eq!(z.geometry(view, native).1, point(px(-55.), px(10.)));
        z.offset = point(px(-500.), px(500.));
        assert_eq!(z.geometry(view, native).1, point(px(-200.), px(10.)));
    }
    #[test]
    fn invalid_image_fails_without_rendering() {
        assert!(decode(b"not an image", false).is_err())
    }
}

#[cfg(test)]
mod gallery_tests {
    use super::{Colors, Gallery, Media, Viewer};
    use gpui::{Context, Window, prelude::*};
    #[gpui::test]
    fn mixed_gallery_opens_http_only_after_next_and_explicit_link_click(
        cx: &mut gpui::TestAppContext,
    ) {
        struct Photos(Media);
        impl Render for Photos {
            fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.0.panel(&Colors::new(false), w, cx).unwrap()
            }
        }
        let url = "http://example.org/b.png";
        cx.update(gpui_component::init);
        let (_, w) = cx.add_window_view(|_, cx| {
            let page = crate::state::dummy_page();
            let images = pimate_markdown::parse(&format!("![a](a.png) ![b]({url})"), true).images;
            let g = Gallery::markdown(&page, "answer", images.into());
            let viewer = cx.new(|_| Viewer {
                photo: Some((g, 0)),
                table: None,
            });
            cx.observe(&viewer, |_, _, cx| cx.notify()).detach();
            Photos(Media {
                page,
                files: true,
                viewer,
            })
        });
        assert!(w.debug_bounds("gallery-http").is_none());
        let next = w.debug_bounds("次へ").unwrap().center();
        w.simulate_click(next, Default::default());
        w.run_until_parked();
        let link = w.debug_bounds("gallery-http").unwrap().center();
        w.simulate_click(link, Default::default());
        w.run_until_parked();
        assert_eq!(cx.opened_url().as_deref(), Some(url));
    }
}
