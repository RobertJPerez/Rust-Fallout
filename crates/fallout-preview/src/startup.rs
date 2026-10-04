//! Console stage receipts accompany the window's loading status.

pub fn stage(message: impl std::fmt::Display) {
    eprintln!("[startup] {message}");
}
