//! Separate executable: the application, not the core, owns the global logger.
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};
/// Counts records that leak a secret. DEBUG/INFO diagnostics are allowed; secrets never.
struct TraceAll(AtomicUsize);
impl log::Log for TraceAll {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        let line = format!("{} {}", record.target(), record.args());
        if line.contains("fake-bearer") || line.contains("fake-key") {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn flush(&self) {}
}
#[tokio::test]
async fn application_trace_logger_cannot_observe_auth_frames() {
    static LOGGER: TraceAll = TraceAll(AtomicUsize::new(0));
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let host = tokio::spawn(async move {
        let mut ws = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        for _ in 0..2 {
            let m = ws.next().await.unwrap().unwrap();
            assert!(m.is_text());
            ws.send(m).await.unwrap();
        }
    });
    let mut app = connect_async(format!("ws://{addr}/ws")).await.unwrap().0;
    for frame in [
        r#"{"type":"auth","token":"fake-bearer"}"#,
        r#"{"type":"rc","request":{"method":"auth_respond","params":{"value":"fake-key"}}}"#,
    ] {
        app.send(Message::Text(frame.into())).await.unwrap();
        assert!(app.next().await.unwrap().unwrap().is_text());
    }
    host.await.unwrap();
    assert_eq!(LOGGER.0.load(Ordering::SeqCst), 0);
}
