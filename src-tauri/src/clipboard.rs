use serde::{Deserialize, Serialize};

const CLIPBOARD_MAX_TEXT_BYTES: usize = 256 * 1024;
// Raw RGBA can be large (a 2560x1440 frame is ~14 MB); cap it so a stray huge
// copy never floods the LAN transport. Images above this are skipped.
const CLIPBOARD_MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClipboardImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba_base64: String,
}

/// One unit of clipboard content read from (or written to) the local system.
#[derive(Debug, Clone)]
pub(crate) enum ClipboardContent {
    Text(String),
    Image(ClipboardImage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(test, target_os = "windows")), allow(dead_code))]
pub(crate) enum ClipboardRead<T> {
    Content(T),
    Unchanged,
    Empty,
    Busy,
    Unsupported,
    Error(String),
}

/// Resolves a bounded series of platform read attempts. Content observed while
/// the platform sequence changes is discarded because it can belong to the old
/// format; busy attempts may retry, while stable empty/error results remain
/// distinguishable and never become an empty remote write.
#[cfg(any(test, target_os = "windows"))]
fn resolve_stable_read<T>(
    attempts: impl IntoIterator<Item = (u64, u64, ClipboardRead<T>)>,
    max_attempts: usize,
) -> ClipboardRead<T> {
    let mut attempted = 0;
    for (before, after, result) in attempts {
        if attempted >= max_attempts {
            break;
        }
        attempted += 1;
        if before != after || matches!(result, ClipboardRead::Busy) {
            continue;
        }
        return result;
    }
    ClipboardRead::Busy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardContentHint {
    Image,
    Text,
    Unknown,
}

fn clipboard_signature_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

impl ClipboardContent {
    pub(crate) fn is_oversized(&self) -> bool {
        match self {
            ClipboardContent::Text(text) => text.len() > CLIPBOARD_MAX_TEXT_BYTES,
            ClipboardContent::Image(image) => {
                // base64 inflates ~4/3; compare against the decoded RGBA budget.
                image.rgba_base64.len() / 4 * 3 > CLIPBOARD_MAX_IMAGE_BYTES
            }
        }
    }

    /// A stable fingerprint used to detect "did the clipboard change" and to
    /// suppress echoing content we just received from a peer.
    pub(crate) fn signature(&self) -> String {
        match self {
            ClipboardContent::Text(text) => format!("text:{text}"),
            ClipboardContent::Image(image) => {
                format!(
                    "image:{}x{}:{}:{:016x}",
                    image.width,
                    image.height,
                    image.rgba_base64.len(),
                    clipboard_signature_hash(image.rgba_base64.as_bytes())
                )
            }
        }
    }
}

pub(crate) fn read_text() -> Result<String, String> {
    read_system_text()
}

pub(crate) fn write_text(text: &str) -> Result<(), String> {
    write_system_text(text)
}

pub(crate) fn write_content(content: &ClipboardContent) -> Result<(), String> {
    match content {
        ClipboardContent::Text(text) => write_text(text),
        ClipboardContent::Image(image) => write_image(image),
    }
}

/// Reads whatever is currently on the clipboard. The shared policy lives here:
/// when the platform can identify a current image format, wait for an image
/// read instead of falling back to stale text from a previous clipboard format.
pub(crate) fn read_content_typed() -> ClipboardRead<ClipboardContent> {
    #[cfg(target_os = "windows")]
    {
        return read_windows_content();
    }

    #[cfg(not(target_os = "windows"))]
    match read_content_for_hint(content_hint(), read_text_content, read_image_content) {
        Some(content) => ClipboardRead::Content(content),
        None => ClipboardRead::Empty,
    }
}

fn read_content_for_hint<F, G>(
    hint: ClipboardContentHint,
    mut read_text: F,
    mut read_image: G,
) -> Option<ClipboardContent>
where
    F: FnMut() -> Option<ClipboardContent>,
    G: FnMut() -> Option<ClipboardContent>,
{
    match hint {
        ClipboardContentHint::Image => read_image(),
        ClipboardContentHint::Text => read_text(),
        ClipboardContentHint::Unknown => read_unknown_content(read_text, read_image),
    }
}

fn read_text_content() -> Option<ClipboardContent> {
    read_text()
        .ok()
        .filter(|text| !text.is_empty())
        .map(ClipboardContent::Text)
}

fn read_image_content() -> Option<ClipboardContent> {
    read_image().map(ClipboardContent::Image)
}

#[cfg(target_os = "windows")]
fn read_unknown_content<F, G>(read_text: F, mut read_image: G) -> Option<ClipboardContent>
where
    F: FnMut() -> Option<ClipboardContent>,
    G: FnMut() -> Option<ClipboardContent>,
{
    read_image().or_else(read_text)
}

#[cfg(not(target_os = "windows"))]
fn read_unknown_content<F, G>(mut read_text: F, read_image: G) -> Option<ClipboardContent>
where
    F: FnMut() -> Option<ClipboardContent>,
    G: FnMut() -> Option<ClipboardContent>,
{
    read_text().or_else(read_image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unknown_clipboard_prefers_text_before_image() {
        let content = read_content_for_hint(
            ClipboardContentHint::Unknown,
            || Some(ClipboardContent::Text("中文测试 abc 123".into())),
            || {
                Some(ClipboardContent::Image(ClipboardImage {
                    width: 1,
                    height: 1,
                    rgba_base64: "AAAAAA==".into(),
                }))
            },
        );

        match content {
            Some(ClipboardContent::Text(text)) => assert_eq!(text, "中文测试 abc 123"),
            _ => panic!("expected text to win when the platform cannot identify clipboard format"),
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unknown_clipboard_keeps_windows_image_first_fallback() {
        let content = read_content_for_hint(
            ClipboardContentHint::Unknown,
            || Some(ClipboardContent::Text("中文测试 abc 123".into())),
            || {
                Some(ClipboardContent::Image(ClipboardImage {
                    width: 1,
                    height: 1,
                    rgba_base64: "AAAAAA==".into(),
                }))
            },
        );

        match content {
            Some(ClipboardContent::Image(image)) => assert_eq!(image.width, 1),
            _ => panic!("expected Windows fallback to keep image priority"),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a32_text_model_preserves_unicode_newlines_and_long_utf8() {
        let text = format!("中文🙂\r\nline two\n{}", "λ".repeat(16_384));
        let content = ClipboardContent::Text(text.clone());
        let ClipboardContent::Text(round_trip) = content else {
            unreachable!()
        };
        assert_eq!(round_trip.as_bytes(), text.as_bytes());
        assert!(!ClipboardContent::Text(text).is_oversized());
    }

    #[test]
    fn a33_typed_clipboard_failures_never_become_empty_content() {
        for status in [
            ClipboardRead::<String>::Unchanged,
            ClipboardRead::<String>::Empty,
            ClipboardRead::Unsupported,
            ClipboardRead::Error("access denied".into()),
        ] {
            let resolved = resolve_stable_read([(7, 7, status.clone())], 3);
            assert_eq!(resolved, status);
            assert!(!matches!(resolved, ClipboardRead::Content(_)));
        }
        assert_eq!(
            resolve_stable_read(
                [
                    (7, 7, ClipboardRead::Busy),
                    (7, 7, ClipboardRead::Content("current".to_string())),
                ],
                3,
            ),
            ClipboardRead::Content("current".to_string())
        );
        assert_eq!(
            resolve_stable_read(
                [
                    (7, 8, ClipboardRead::Content("stale text".to_string())),
                    (8, 8, ClipboardRead::Unsupported),
                ],
                3,
            ),
            ClipboardRead::Unsupported,
            "a format change must not fall back to text from the old sequence"
        );
        assert_eq!(
            resolve_stable_read(
                [
                    (1, 1, ClipboardRead::<String>::Busy),
                    (1, 1, ClipboardRead::Busy),
                    (1, 1, ClipboardRead::Busy),
                    (1, 1, ClipboardRead::Content("too late".into())),
                ],
                3,
            ),
            ClipboardRead::Busy,
            "the retry budget must be hard bounded"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_change_count_reads_only_after_a_new_revision() {
        let mut last = 41;
        assert!(!macos_change_observed(&mut last, 41));
        assert!(macos_change_observed(&mut last, 42));
        assert_eq!(last, 42);
        assert!(!macos_change_observed(&mut last, 42));
    }
}

#[cfg(target_os = "macos")]
fn macos_change_observed(last: &mut i64, current: i64) -> bool {
    if *last == current {
        false
    } else {
        *last = current;
        true
    }
}

#[cfg(target_os = "macos")]
pub(crate) struct MacClipboardWatcher {
    last_change_count: i64,
}

#[cfg(target_os = "macos")]
impl MacClipboardWatcher {
    pub(crate) fn start() -> Self {
        Self {
            last_change_count: macos_clipboard_change_count(),
        }
    }

    pub(crate) fn discard_pending_change(&mut self) {
        self.last_change_count = macos_clipboard_change_count();
    }

    pub(crate) fn wait_for_change(&mut self, timeout: std::time::Duration) -> bool {
        std::thread::sleep(timeout);
        macos_change_observed(&mut self.last_change_count, macos_clipboard_change_count())
    }
}

#[cfg(target_os = "macos")]
fn macos_clipboard_change_count() -> i64 {
    use objc2_app_kit::NSPasteboard;

    NSPasteboard::generalPasteboard().changeCount() as i64
}

fn read_image() -> Option<ClipboardImage> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    let arboard_image = arboard::Clipboard::new().ok().and_then(|mut clipboard| {
        let image = clipboard.get_image().ok()?;
        if image.width == 0 || image.height == 0 || image.bytes.is_empty() {
            return None;
        }
        if image.bytes.len() > CLIPBOARD_MAX_IMAGE_BYTES {
            return None;
        }

        Some(ClipboardImage {
            width: image.width as u32,
            height: image.height as u32,
            rgba_base64: BASE64.encode(image.bytes.as_ref()),
        })
    });

    arboard_image.or_else(|| {
        #[cfg(target_os = "windows")]
        {
            read_windows_dib_image()
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    })
}

fn write_image(image: &ClipboardImage) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    let bytes = BASE64
        .decode(image.rgba_base64.as_bytes())
        .map_err(|error| format!("failed to decode clipboard image: {error}"))?;
    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 || bytes.len() != width.saturating_mul(height).saturating_mul(4) {
        return Err("clipboard image has invalid dimensions".into());
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to open clipboard: {error}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width,
            height,
            bytes: std::borrow::Cow::Owned(bytes),
        })
        .map_err(|error| format!("failed to write clipboard image: {error}"))
}

#[cfg(target_os = "windows")]
fn content_hint() -> ClipboardContentHint {
    use windows_sys::Win32::System::DataExchange::{
        IsClipboardFormatAvailable, RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Ole::{CF_BITMAP, CF_DIB, CF_DIBV5, CF_UNICODETEXT};

    let png_format = unsafe { RegisterClipboardFormatW(crate::wide_null("PNG").as_ptr()) };
    let image_formats = [
        png_format,
        u32::from(CF_DIBV5),
        u32::from(CF_DIB),
        u32::from(CF_BITMAP),
    ];
    if image_formats
        .iter()
        .any(|format| *format != 0 && unsafe { IsClipboardFormatAvailable(*format) } != 0)
    {
        return ClipboardContentHint::Image;
    }
    if unsafe { IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT)) } != 0 {
        ClipboardContentHint::Text
    } else {
        ClipboardContentHint::Unknown
    }
}

#[cfg(not(target_os = "windows"))]
fn content_hint() -> ClipboardContentHint {
    ClipboardContentHint::Unknown
}

#[cfg(target_os = "windows")]
fn read_windows_dib_image() -> Option<ClipboardImage> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5};

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return None;
    }
    let _guard = ClipboardGuard;

    for format in [u32::from(CF_DIBV5), u32::from(CF_DIB)] {
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            continue;
        }
        let len = unsafe { GlobalSize(handle) };
        if len == 0 || len > CLIPBOARD_MAX_IMAGE_BYTES.saturating_add(256) {
            continue;
        }
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            continue;
        }
        let data = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
        let decoded = decode_windows_dib_image(data);
        unsafe {
            let _ = GlobalUnlock(handle);
        }
        if decoded.is_some() {
            return decoded;
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn decode_windows_dib_image(data: &[u8]) -> Option<ClipboardImage> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use image::{codecs::bmp::BmpDecoder, DynamicImage, ImageDecoder};

    let decoder = BmpDecoder::new_without_file_header(std::io::Cursor::new(data)).ok()?;
    let (width, height) = decoder.dimensions();
    let rgba = DynamicImage::from_decoder(decoder).ok()?.into_rgba8();
    let bytes = rgba.into_raw();
    if width == 0 || height == 0 || bytes.is_empty() || bytes.len() > CLIPBOARD_MAX_IMAGE_BYTES {
        return None;
    }

    Some(ClipboardImage {
        width,
        height,
        rgba_base64: BASE64.encode(bytes),
    })
}

#[cfg(target_os = "windows")]
fn read_windows_content() -> ClipboardRead<ClipboardContent> {
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

    const MAX_ATTEMPTS: usize = 3;
    for _ in 0..MAX_ATTEMPTS {
        let before = unsafe { GetClipboardSequenceNumber() } as u64;
        let result = match content_hint() {
            ClipboardContentHint::Text => read_windows_text_once().map(ClipboardContent::Text),
            ClipboardContentHint::Image => read_image()
                .map(ClipboardContent::Image)
                .map_or(ClipboardRead::Busy, ClipboardRead::Content),
            ClipboardContentHint::Unknown => ClipboardRead::Unsupported,
        };
        let after = unsafe { GetClipboardSequenceNumber() } as u64;
        match resolve_stable_read([(before, after, result)], 1) {
            ClipboardRead::Busy => std::thread::sleep(std::time::Duration::from_millis(4)),
            stable => return stable,
        }
    }
    ClipboardRead::Busy
}

#[cfg(target_os = "windows")]
impl<T> ClipboardRead<T> {
    fn map<U>(self, map: impl FnOnce(T) -> U) -> ClipboardRead<U> {
        match self {
            ClipboardRead::Content(value) => ClipboardRead::Content(map(value)),
            ClipboardRead::Unchanged => ClipboardRead::Unchanged,
            ClipboardRead::Empty => ClipboardRead::Empty,
            ClipboardRead::Busy => ClipboardRead::Busy,
            ClipboardRead::Unsupported => ClipboardRead::Unsupported,
            ClipboardRead::Error(error) => ClipboardRead::Error(error),
        }
    }
}

#[cfg(target_os = "windows")]
fn read_windows_text_once() -> ClipboardRead<String> {
    use windows_sys::Win32::System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    };

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return ClipboardRead::Busy;
    }
    let _guard = ClipboardGuard;
    let handle = unsafe { GetClipboardData(u32::from(CF_UNICODETEXT)) };
    if handle.is_null() {
        return ClipboardRead::Unsupported;
    }
    let byte_len = unsafe { GlobalSize(handle) };
    if byte_len == 0 {
        return ClipboardRead::Empty;
    }
    if byte_len > CLIPBOARD_MAX_TEXT_BYTES.saturating_mul(2).saturating_add(2) {
        return ClipboardRead::Error("clipboard text exceeds the configured byte limit".into());
    }
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        return ClipboardRead::Busy;
    }
    let units = unsafe {
        std::slice::from_raw_parts(pointer.cast::<u16>(), byte_len / std::mem::size_of::<u16>())
    };
    let terminator = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let result = if terminator == 0 {
        ClipboardRead::Empty
    } else {
        String::from_utf16(&units[..terminator])
            .map(ClipboardRead::Content)
            .unwrap_or_else(|error| {
                ClipboardRead::Error(format!("invalid UTF-16 clipboard text: {error}"))
            })
    };
    unsafe {
        let _ = GlobalUnlock(handle);
    }
    result
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsClipboardListener {
    window: isize,
    thread_id: u32,
    changes: std::sync::mpsc::Receiver<u64>,
    thread: Option<std::thread::JoinHandle<()>>,
    last_sequence: u64,
}

#[cfg(target_os = "windows")]
impl WindowsClipboardListener {
    pub(crate) fn start() -> Result<Self, String> {
        use std::sync::mpsc;
        use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

        let (change_tx, changes) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("mykvm-clipboard-listener".into())
            .spawn(move || windows_clipboard_message_loop(change_tx, ready_tx))
            .map_err(|error| format!("failed to start clipboard listener thread: {error}"))?;
        let (window, thread_id) = match ready_rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(ready)) => ready,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(_) => return Err("clipboard listener did not initialize in time".into()),
        };
        Ok(Self {
            window,
            thread_id,
            changes,
            thread: Some(thread),
            last_sequence: unsafe { GetClipboardSequenceNumber() } as u64,
        })
    }

    pub(crate) fn wait_for_change(&mut self, timeout: std::time::Duration) -> ClipboardRead<u64> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match self.changes.recv_timeout(remaining) {
                Ok(sequence) if sequence == self.last_sequence => {
                    if std::time::Instant::now() >= deadline {
                        return ClipboardRead::Unchanged;
                    }
                }
                Ok(sequence) => {
                    self.last_sequence = sequence;
                    return ClipboardRead::Content(sequence);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return ClipboardRead::Unchanged;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return ClipboardRead::Error("clipboard listener stopped".into());
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsClipboardListener {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            PostMessageW, PostThreadMessageW, WM_CLOSE, WM_QUIT,
        };
        let posted = unsafe { PostMessageW(self.window as _, WM_CLOSE, 0, 0) };
        if posted == 0 {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
struct WindowsClipboardListenerContext {
    changes: std::sync::mpsc::Sender<u64>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn windows_clipboard_window_proc(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        System::DataExchange::{GetClipboardSequenceNumber, RemoveClipboardFormatListener},
        UI::WindowsAndMessaging::{
            DefWindowProcW, DestroyWindow, GetWindowLongPtrW, PostQuitMessage, SetWindowLongPtrW,
            CREATESTRUCTW, GWLP_USERDATA, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_NCCREATE, WM_NCDESTROY,
        },
    };

    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        return 1;
    }
    let context = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowsClipboardListenerContext;
    match message {
        WM_CLIPBOARDUPDATE => {
            if let Some(context) = context.as_ref() {
                let _ = context.changes.send(GetClipboardSequenceNumber() as u64);
            }
            0
        }
        WM_CLOSE => {
            let _ = RemoveClipboardFormatListener(window);
            let _ = DestroyWindow(window);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            if !context.is_null() {
                drop(Box::from_raw(context));
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

#[cfg(target_os = "windows")]
fn windows_clipboard_message_loop(
    changes: std::sync::mpsc::Sender<u64>,
    ready: std::sync::mpsc::Sender<Result<(isize, u32), String>>,
) {
    use windows_sys::Win32::{
        System::{
            DataExchange::AddClipboardFormatListener, LibraryLoader::GetModuleHandleW,
            Threading::GetCurrentThreadId,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, IsWindow,
            RegisterClassW, TranslateMessage, UnregisterClassW, HWND_MESSAGE, MSG, WNDCLASSW,
        },
    };

    let class_name = crate::wide_null(&format!(
        "MyKVM_Local_Clipboard_Listener_{}",
        std::process::id()
    ));
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(windows_clipboard_window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let _ = ready.send(Err("failed to register clipboard listener window".into()));
        return;
    }
    let context = Box::into_raw(Box::new(WindowsClipboardListenerContext { changes }));
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            context.cast(),
        )
    };
    if window.is_null() {
        unsafe {
            drop(Box::from_raw(context));
            let _ = UnregisterClassW(class_name.as_ptr(), instance);
        }
        let _ = ready.send(Err("failed to create clipboard listener window".into()));
        return;
    }
    if unsafe { AddClipboardFormatListener(window) } == 0 {
        unsafe {
            let _ = DestroyWindow(window);
            let _ = UnregisterClassW(class_name.as_ptr(), instance);
        }
        let _ = ready.send(Err("failed to subscribe to clipboard updates".into()));
        return;
    }
    if ready
        .send(Ok((window as isize, unsafe { GetCurrentThreadId() })))
        .is_err()
    {
        unsafe {
            let _ = DestroyWindow(window);
            let _ = UnregisterClassW(class_name.as_ptr(), instance);
        }
        return;
    }

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        if IsWindow(window) != 0 {
            let _ = windows_sys::Win32::System::DataExchange::RemoveClipboardFormatListener(window);
            let _ = DestroyWindow(window);
        }
        let _ = UnregisterClassW(class_name.as_ptr(), instance);
    }
}

#[cfg(target_os = "windows")]
fn read_system_text() -> Result<String, String> {
    match read_windows_text_once() {
        ClipboardRead::Content(text) => Ok(text),
        ClipboardRead::Empty => Ok(String::new()),
        ClipboardRead::Busy => Err("clipboard is busy".into()),
        ClipboardRead::Unsupported => Err("clipboard does not contain Unicode text".into()),
        ClipboardRead::Error(error) => Err(error),
        ClipboardRead::Unchanged => Err("clipboard did not change".into()),
    }
}

#[cfg(target_os = "macos")]
fn read_system_text() -> Result<String, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to open clipboard: {error}"))?;
    clipboard
        .get_text()
        .map_err(|error| format!("failed to read clipboard text: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn read_system_text() -> Result<String, String> {
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            "wl-paste -n 2>/dev/null || xclip -selection clipboard -out",
        ])
        .output()
        .map_err(|error| format!("failed to read clipboard: {error}"))?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|error| format!("clipboard text is not valid UTF-8: {error}"))
    } else {
        Err(format!(
            "clipboard command exited with status {}",
            output.status
        ))
    }
}

#[cfg(target_os = "windows")]
fn write_system_text(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to open clipboard: {error}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| format!("failed to write clipboard text: {error}"))
}

#[cfg(target_os = "macos")]
fn write_system_text(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("failed to open clipboard: {error}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| format!("failed to write clipboard text: {error}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn write_system_text(text: &str) -> Result<(), String> {
    use std::{io::Write, process::Command, process::Stdio};

    let mut child = Command::new("sh")
        .args(["-c", "wl-copy 2>/dev/null || xclip -selection clipboard"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to write clipboard: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("failed to send clipboard text: {error}"))?;
    }

    let status = child
        .wait()
        .map_err(|error| format!("failed to finish clipboard write: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("clipboard command exited with status {status}"))
    }
}
