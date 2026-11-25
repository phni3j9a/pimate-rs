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

pub(super) fn encode_file(path: &Path, budget: usize) -> Result<p::PromptImage, Error> {
    use image::{DynamicImage, ImageDecoder, ImageReader};
    const SOURCE: u64 = 32 * 1024 * 1024;
    let meta = path.metadata().map_err(|_| Error::BadRequest)?;
    if !meta.is_file() || meta.len() > SOURCE {
        return Err(Error::BadRequest);
    }
    let mut bytes = vec![];
    std::fs::File::open(path)
        .and_then(|f| f.take(SOURCE + 1).read_to_end(&mut bytes))
        .map_err(|_| Error::BadRequest)?;
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
#[derive(PartialEq, Eq)]
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
pub(super) struct Images {
    cache: tokio::sync::Mutex<Cache>,
    waiting: tokio::sync::Semaphore,
}
impl Default for Images {
    fn default() -> Self {
        Self {
            cache: Default::default(),
            waiting: tokio::sync::Semaphore::new(16),
        }
    }
}
impl Page {
    pub async fn image(&self, s: Source<'_>, thumbnail: bool) -> Result<Arc<[u8]>, Error> {
        self.editor()?;
        let _slot = self
            .images
            .waiting
            .try_acquire()
            .map_err(|_| Error::Blocked)?;
        let mut cache = self.images.cache.lock().await;
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
        if let Some(v) = cache.hit(&id, thumbnail) {
            return Ok(v);
        }
        let data = match s {
            Source::Entry(k, n) => image_data(self.entry_image(k, n, thumbnail).await?)?,
            Source::File { path, .. } => image_data(self.file_image(path, thumbnail).await?)?,
            Source::Inline(b) => decode(b)?,
            Source::Https(u) => download(u).await?,
        };
        self.current()?;
        let bytes: Arc<[u8]> = data.into();
        cache.put(id, thumbnail, bytes.clone());
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
    let task = rt.spawn(async move {
        tokio::time::timeout(Duration::from_secs(20), fetch(url))
            .await
            .map_err(|_| Error::Timeout)?
    });
    struct Abort(tokio::task::AbortHandle);
    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort()
        }
    }
    let _abort = Abort(task.abort_handle());
    task.await.map_err(|_| Error::Offline)?
}
