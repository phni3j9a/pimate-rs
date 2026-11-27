//! Native clipboard smoke check. Run only in a disposable GUI session with the fixture offer.
fn main() {
    gpui::Application::new().run(|cx| {
        assert!(cx.clipboard_image_reader().is_none());
        assert_eq!(
            cx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
            Some("PiMate clipboard text")
        );
        std::process::exit(0); // No event loop is needed after the two native reads.
    });
}
