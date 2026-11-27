use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use pimate_protocol as p;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{Cursor, Read},
    path::Path,
    time::Duration,
};
const WEB_MAX: usize = 8 * 1024 * 1024;

pub(crate) const SOURCE: u64 = 32 * 1024 * 1024;
pub(super) fn encode_file(path: &Path, budget: usize) -> Result<p::PromptImage, Error> {
    let meta = path.metadata().map_err(|_| Error::BadRequest)?;
    if !meta.is_file() || meta.len() > SOURCE {
        return Err(Error::BadRequest);
    }
    let mut bytes = vec![];
    std::fs::File::open(path)
        .and_then(|f| f.take(SOURCE + 1).read_to_end(&mut bytes))
        .map_err(|_| Error::BadRequest)?;
    encode_bytes(&bytes, budget)
}
pub(super) fn encode_bytes(bytes: &[u8], budget: usize) -> Result<p::PromptImage, Error> {
    use image::{DynamicImage, ImageDecoder, ImageReader};
    if bytes.len() as u64 > SOURCE {
        return Err(Error::BadRequest);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::BadRequest)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(96 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| Error::BadRequest)?;
    if decoder.total_bytes() > 96 * 1024 * 1024 {
        return Err(Error::BadRequest);
    }
    let orientation = decoder.orientation().map_err(|_| Error::BadRequest)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| Error::BadRequest)?;
    img.apply_orientation(orientation);
    for edge in [1600, 1280, 960, 640, 400, 240] {
        let small = if img.width().max(img.height()) > edge {
            img.thumbnail(edge, edge).to_rgb8()
        } else {
            img.to_rgb8()
        };
        let mut jpeg = vec![];
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 78)
            .encode_image(&small)
            .map_err(|_| Error::BadRequest)?;
        if jpeg.len() <= budget {
            return Ok(p::PromptImage {
                r#type: p::PromptImageType::Image,
                mime_type: p::PromptImageMimeType::ImageJpeg,
                data: STANDARD.encode(jpeg),
            });
        }
    }
    Err(Error::BadRequest)
}

pub enum Source<'a> {
    Entry(&'a str, u64),
    File { answer: &'a str, path: &'a str },
    Inline(&'a str),
    Https(Url),
}
impl<'a> Source<'a> {
    pub fn markdown(i: &'a pimate_markdown::Image, answer: &'a str) -> Result<Self, Error> {
        use pimate_markdown::ImageKind::*;
        Ok(match i.kind {
            File => Self::File {
                answer,
                path: &i.src,
            },
            Data => Self::Inline(i.src.split_once(',').ok_or(Error::BadRequest)?.1),
            Remote => Self::Https(https(Url::parse(&i.src).map_err(|_| Error::BadRequest)?)?),
        })
    }
}
pub fn history_count(row: &Value) -> u64 {
    row["imageCount"]
        .as_u64()
        .unwrap_or(0)
        .max(row["images"].as_array().map_or(0, |v| v.len() as u64))
}
pub fn history_source<'a>(
    entry: &'a str,
    row: &'a Value,
    index: u64,
    thumbnail: bool,
    fetch: bool,
) -> Option<Source<'a>> {
    if index >= history_count(row) {
        return None;
    }
    if thumbnail || !fetch {
        for (i, v) in row["images"].as_array().into_iter().flatten().enumerate() {
            let slot = if row["imageIndices"].is_array() {
                row["imageIndices"][i].as_u64()
            } else {
                Some(i as u64)
            };
            if slot == Some(index)
                && let Some(s) = v["data"].as_str()
            {
                return Some(Source::Inline(s));
            }
        }
    }
    fetch.then_some(Source::Entry(entry, index))
}
#[derive(Clone, Hash, PartialEq, Eq)]
enum ImageId {
    Entry(String, u64),
    File(String, String),
    Inline([u8; 32]),
    Https(String),
}
#[derive(Default)]
struct Cache(VecDeque<(ImageId, bool, Arc<[u8]>)>);
impl Cache {
    fn hit(&mut self, id: &ImageId, t: bool) -> Option<Arc<[u8]>> {
        let i = self
            .0
            .iter()
            .position(|(k, thumb, _)| k == id && *thumb == t)?;
        let item = self.0.remove(i)?;
        let bytes = item.2.clone();
        self.0.push_back(item);
        Some(bytes)
    }
    fn put(&mut self, id: ImageId, t: bool, b: Arc<[u8]>) {
        self.0.push_back((id, t, b));
        while self.0.len() > 64
            || self.0.iter().map(|(_, _, b)| b.len()).sum::<usize>() > 12 * 1024 * 1024
        {
            self.0.pop_front();
        }
    }
}
type Flight = tokio::sync::OnceCell<Result<Arc<[u8]>, Error>>;
#[derive(Default)]
struct Loads {
    cache: Cache,
    pending: std::collections::HashMap<(ImageId, bool), Weak<Flight>>,
}
pub(super) struct Images {
    loads: std::sync::Mutex<Loads>,
    waiting: tokio::sync::Semaphore,
}
impl Default for Images {
    fn default() -> Self {
        Self {
            loads: Default::default(),
            waiting: tokio::sync::Semaphore::new(16),
        }
    }
}
impl Images {
    async fn load(
        &self,
        id: ImageId,
        t: bool,
        fetch: impl std::future::Future<Output = Result<Vec<u8>, Error>>,
    ) -> Result<Arc<[u8]>, Error> {
        let key = (id, t);
        let (flight, _slot) = {
            let mut loads = self.loads.lock().unwrap();
            if let Some(hit) = loads.cache.hit(&key.0, t) {
                return Ok(hit);
            }
            let slot = self.waiting.try_acquire().map_err(|_| Error::Blocked)?;
            loads.pending.retain(|_, v| v.strong_count() > 0);
            let flight = loads
                .pending
                .get(&key)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| {
                    let cell = Arc::new(Flight::new());
                    loads.pending.insert(key.clone(), Arc::downgrade(&cell));
                    cell
                });
            (flight, slot)
        };
        flight
            .get_or_init(|| async {
                let bytes: Arc<[u8]> = fetch.await?.into();
                self.loads
                    .lock()
                    .unwrap()
                    .cache
                    .put(key.0, t, bytes.clone());
                Ok(bytes)
            })
            .await
            .clone()
    }
}
impl Page {
    pub async fn image(&self, s: Source<'_>, thumbnail: bool) -> Result<Arc<[u8]>, Error> {
        self.editor()?;
        self.current()?;
        let id = match &s {
            Source::Entry(k, n) => ImageId::Entry((*k).into(), *n),
            Source::File { answer, path } => ImageId::File((*answer).into(), (*path).into()),
            Source::Inline(s) => {
                if s.len() > 12 * 1024 * 1024 {
                    return Err(Error::BadRequest);
                }
                ImageId::Inline(Sha256::digest(s.as_bytes()).into())
            }
            Source::Https(u) => ImageId::Https(u.as_str().into()),
        };
        let bytes = self
            .images
            .load(id, thumbnail, async {
                let data = match s {
                    Source::Entry(k, n) => image_data(self.entry_image(k, n, thumbnail).await?)?,
                    Source::File { path, .. } => {
                        image_data(self.file_image(path, thumbnail).await?)?
                    }
                    Source::Inline(b) => decode(b)?,
                    Source::Https(u) => download(u).await?,
                };
                self.current()?;
                Ok(data)
            })
            .await?;
        self.live()?;
        Ok(bytes)
    }
}
fn image_data(v: Value) -> Result<Vec<u8>, Error> {
    decode(v["image"]["data"].as_str().ok_or(Error::InvalidMessage)?)
}
fn decode(s: &str) -> Result<Vec<u8>, Error> {
    if s.is_empty() || s.len() > 12 * 1024 * 1024 {
        return Err(Error::BadRequest);
    }
    STANDARD.decode(s).map_err(|_| Error::BadRequest)
}
fn https(u: Url) -> Result<Url, Error> {
    if u.scheme() == "https" && u.has_host() && u.username().is_empty() && u.password().is_none() {
        Ok(u)
    } else {
        Err(Error::BadRequest)
    }
}
fn redirect(u: &Url, location: &str, count: usize) -> Result<Url, Error> {
    if count >= 5 {
        return Err(Error::BadRequest);
    }
    https(u.join(location).map_err(|_| Error::BadRequest)?)
}
fn append(bytes: &mut Vec<u8>, chunk: &[u8], limit: usize) -> Result<(), Error> {
    if chunk.len() > limit.saturating_sub(bytes.len()) {
        return Err(Error::BadRequest);
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}
async fn fetch(mut url: Url) -> Result<Vec<u8>, Error> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .referer(false)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::Offline)?;
    for count in 0..=5 {
        let mut r = client
            .get(https(url.clone())?)
            .send()
            .await
            .map_err(|_| Error::Offline)?;
        if matches!(r.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            url = redirect(
                &url,
                r.headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|h| h.to_str().ok())
                    .ok_or(Error::BadRequest)?,
                count,
            )?;
            continue;
        }
        if !r.status().is_success() || r.content_length().is_some_and(|n| n > WEB_MAX as u64) {
            return Err(Error::BadRequest);
        }
        let mut out = vec![];
        while let Some(b) = r.chunk().await.map_err(|_| Error::Offline)? {
            append(&mut out, &b, WEB_MAX)?;
        }
        if out.is_empty() {
            return Err(Error::BadRequest);
        }
        return Ok(out);
    }
    Err(Error::BadRequest)
}
async fn deadline<T>(
    limit: Duration,
    f: impl std::future::Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::time::timeout(limit, f)
        .await
        .map_err(|_| Error::Timeout)?
}
async fn download(url: Url) -> Result<Vec<u8>, Error> {
    // GPUI's executor is not a Tokio reactor. Abort the one ephemeral request when its tile closes.
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let rt = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("image runtime")
    });
    let task = rt.spawn(deadline(Duration::from_secs(20), fetch(url)));
    struct Abort(tokio::task::AbortHandle);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort()
        }
    }
    let _abort = Abort(task.abort_handle());
    task.await.map_err(|_| Error::Offline)?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_validate_every_hop_and_streamed_bytes() {
        let u = Url::parse("https://example.test/a").unwrap();
        assert!(redirect(&u, "/safe", 4).is_ok());
        for (v, n) in [
            ("http://example.test/x", 0),
            ("//user:pass@example.test", 1),
            ("file:///tmp/x", 0),
            ("/sixth", 5),
        ] {
            assert!(redirect(&u, v, n).is_err())
        }
        let mut out = vec![];
        assert!(append(&mut out, b"123", 4).is_ok());
        assert!(append(&mut out, b"4", 4).is_ok());
        assert!(append(&mut out, b"5", 4).is_err());
        assert_eq!(out, b"1234");
    }
    #[tokio::test]
    async fn even_an_unending_transfer_has_one_deadline() {
        let start = std::time::Instant::now();
        let result = deadline(Duration::from_millis(20), async {
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await
            }
            #[allow(unreachable_code)]
            Ok(())
        })
        .await;
        assert_eq!(result, Err(Error::Timeout));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn sparse_slots_do_not_allocate_by_count() {
        let r = serde_json::json!({"imageCount":100_000_000,"images":[{"data":"eA=="}],"imageIndices":[2]});
        assert_eq!(history_count(&r), 100_000_000);
        assert!(matches!(
            history_source("e", &r, 2, true, true),
            Some(Source::Inline("eA=="))
        ));
        assert!(matches!(
            history_source("e", &r, 99_999_999, false, true),
            Some(Source::Entry("e", 99_999_999))
        ));
        assert!(history_source("e", &r, 0, true, false).is_none());
    }
    #[tokio::test]
    async fn cached_hit_does_not_queue_behind_an_unrelated_load_and_same_key_joins() {
        let images = Images::default();
        let key = |id| ImageId::Entry("entry".into(), id);
        images
            .load(key(1), true, async { Ok(vec![1]) })
            .await
            .unwrap();
        let (release, gate) = tokio::sync::oneshot::channel();
        let mut slow = Box::pin(images.load(key(2), true, async {
            gate.await.unwrap();
            Ok(vec![2])
        }));
        assert!(futures_util::poll!(&mut slow).is_pending());
        let mut shared = Box::pin(images.load(key(2), true, async { panic!("duplicate fetch") }));
        assert!(futures_util::poll!(&mut shared).is_pending());
        let slots = images.waiting.try_acquire_many(14).unwrap();
        let hit = images.load(key(1), true, async { panic!("cache miss") });
        assert!(
            matches!(futures_util::poll!(Box::pin(hit)),std::task::Poll::Ready(Ok(v)) if *v==[1])
        );
        drop(slots);
        assert_eq!(
            &*images
                .load(key(3), true, async { Ok(vec![3]) })
                .await
                .unwrap(),
            &[3]
        );
        release.send(()).unwrap();
        assert!(Arc::ptr_eq(&slow.await.unwrap(), &shared.await.unwrap()));
    }
    #[test]
    fn cache_hit_refreshes_age_and_files_are_answer_scoped() {
        let mut c = Cache::default();
        let key = |n: usize| ImageId::File(n.to_string(), "same.png".into());
        for n in 0..64 {
            c.put(key(n), true, Arc::from([1]));
        }
        assert!(c.hit(&key(0), true).is_some());
        c.put(key(64), true, Arc::from([2]));
        assert!(c.hit(&key(0), true).is_some());
        assert!(c.hit(&key(1), true).is_none());
        assert!(c.hit(&key(64), false).is_none());
    }
    #[test]
    fn picked_file_is_encoded_as_bounded_jpeg() {
        let p = std::env::temp_dir().join(format!("{}.png", uuid::Uuid::new_v4()));
        image::RgbImage::new(50, 100).save(&p).unwrap();
        let r = encode_file(&p, 1024).unwrap();
        assert_eq!(r.mime_type, p::PromptImageMimeType::ImageJpeg);
        assert!(STANDARD.decode(r.data).unwrap().len() <= 1024);
        assert!(encode_file(&p, 2).is_err());
        std::fs::remove_file(p).unwrap();
    }
}
