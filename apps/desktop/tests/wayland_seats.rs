//! Exercise the vendored GPUI seat policy without a compositor or a physical second seat.
#[path = "../../../vendor/gpui/src/platform/linux/wayland/keyboard_seat.rs"]
mod keyboard_seat;

#[test]
fn select_by_capability_and_preserve_the_composing_seat() {
    for seats in [
        [(1, 'A', true), (2, 'B', false)],
        [(2, 'B', false), (1, 'A', true)],
    ] {
        assert_eq!(keyboard_seat::select(&seats, None), Some(&'A'));
        assert_eq!(keyboard_seat::select(&seats, Some(&'A')), Some(&'A'));
    }
    let both = [(1, 'A', true), (2, 'B', true)];
    assert_eq!(keyboard_seat::select(&both, Some(&'B')), Some(&'B'));
    assert_eq!(keyboard_seat::select(&both[..1], Some(&'B')), Some(&'A'));
    assert_eq!(keyboard_seat::select(&[(1, 'A', false)], Some(&'A')), None);
}

#[cfg(unix)]
#[path = "../../../vendor/gpui/src/platform/linux/wayland/image_transfer.rs"]
mod image_transfer;

#[cfg(unix)]
#[test]
fn slow_clipboard_producer_cannot_renew_the_deadline() {
    use std::{
        io::Write,
        os::unix::net::UnixStream,
        time::{Duration, Instant},
    };
    let (read, mut write) = UnixStream::pair().unwrap();
    let producer = std::thread::spawn(move || {
        for _ in 0..30 {
            if write.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    assert!(image_transfer::read(read, 1024, Instant::now() + Duration::from_millis(100)).is_err());
    producer.join().unwrap();
}
