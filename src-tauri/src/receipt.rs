// ==============================================================================
// OWNERSHIP:
//   Desktop notifications: a small card in the bottom-right corner of the
//   screen the user is on, over other apps, with a completion sound.
//   One hidden, click-through window is reused; cards show one at a time.
// ==============================================================================
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

const LABEL: &str = "receipt";
/// Card size and its distance from the work-area corner, in logical pixels.
const WIDTH: f64 = 390.0;
const HEIGHT: f64 = 158.0;
const MARGIN: f64 = 16.0;
/// Transparent space around the card inside the window. WebView2 rounds its
/// drawing surface up to whole device-independent pixels, so at fractional
/// scales (125 %, 150 %) it can overhang the window by a pixel on the right
/// and bottom; an edge-to-edge card loses its border there.
const GUTTER: f64 = 2.0;
const VISIBLE_FOR: Duration = Duration::from_secs(4);
/// The page must paint the new card before the window is shown.
const PAINT_DEADLINE: Duration = Duration::from_secs(3);
const QUEUE_LIMIT: usize = 4;

/// The ZeroFoundry notification sounds.
pub const SOUNDS: &[(&str, &[u8])] = &[
    (
        "silver",
        include_bytes!("../assets/notification-silver.wav"),
    ),
    ("satin", include_bytes!("../assets/notification-satin.wav")),
    ("bloom", include_bytes!("../assets/notification-bloom.wav")),
    ("felt", include_bytes!("../assets/notification-felt.wav")),
];

fn sound(id: &str) -> &'static [u8] {
    SOUNDS
        .iter()
        .find(|(name, _)| *name == id)
        .unwrap_or(&SOUNDS[0])
        .1
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Success,
    Warning,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub tone: Tone,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPrefs {
    pub enabled: bool,
    pub sound: bool,
    /// One of `SOUNDS`.
    pub sound_id: String,
    /// 0..=100
    pub volume: u8,
}

struct Shared {
    queue: Sender<Receipt>,
    painted: Sender<()>,
    pending: std::sync::atomic::AtomicUsize,
    current: Mutex<Option<Receipt>>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Creates the hidden window and the worker. Call once from setup.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(
        app,
        LABEL,
        tauri::WebviewUrl::App("index.html#/receipt".into()),
    )
    .title("ZeroConverter")
    .inner_size(WIDTH + 2.0 * GUTTER, HEIGHT + 2.0 * GUTTER)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .visible(false)
    .build()?
    .set_ignore_cursor_events(true)?;

    let (queue, jobs) = mpsc::channel();
    let (painted, paints) = mpsc::channel();
    let _ = SHARED.set(Shared {
        queue,
        painted,
        pending: Default::default(),
        current: Mutex::new(None),
    });
    let app = app.clone();
    std::thread::Builder::new()
        .name("zc-receipt".into())
        .spawn(move || worker(app, jobs, paints))
        .map(|_| ())
        .map_err(tauri::Error::from)
}

/// Queues a card. Dropped when notifications are off or four are waiting.
pub fn show(app: &AppHandle, receipt: Receipt) {
    if !prefs(app).enabled {
        return;
    }
    let Some(shared) = SHARED.get() else { return };
    use std::sync::atomic::Ordering;
    if shared.pending.load(Ordering::Relaxed) >= QUEUE_LIMIT {
        return;
    }
    shared.pending.fetch_add(1, Ordering::Relaxed);
    let _ = shared.queue.send(receipt);
}

/// The card the page should draw now (asked for once the page loads).
pub fn current() -> Option<Receipt> {
    SHARED.get()?.current.lock().ok()?.clone()
}

/// The page drew the current card.
pub fn painted() {
    if let Some(shared) = SHARED.get() {
        let _ = shared.painted.send(());
    }
}

pub fn prefs(app: &AppHandle) -> NotificationPrefs {
    let s = crate::settings::load(app);
    NotificationPrefs {
        enabled: s.notify,
        sound: s.notify_sound,
        sound_id: s.notify_sound_id,
        volume: s.notify_volume.min(100),
    }
}

pub fn set_prefs(app: &AppHandle, p: NotificationPrefs) -> Result<(), String> {
    crate::settings::save(app, |s| {
        s.notify = p.enabled;
        s.notify_sound = p.sound;
        s.notify_sound_id = p.sound_id;
        s.notify_volume = p.volume.min(100);
    })
}

fn worker(app: AppHandle, jobs: Receiver<Receipt>, paints: Receiver<()>) {
    use std::sync::atomic::Ordering;
    let shared = SHARED.get().expect("set before the worker starts");
    for receipt in jobs {
        shared.pending.fetch_sub(1, Ordering::Relaxed);
        let p = prefs(&app);
        // Respect focus assist / presentation mode, and a switch flipped
        // while this card waited.
        if !p.enabled || !platform::accepts_notifications() {
            continue;
        }
        let Some(window) = app.get_webview_window(LABEL) else {
            continue;
        };
        while paints.try_recv().is_ok() {}
        let tone = receipt.tone;
        *shared.current.lock().unwrap() = Some(receipt.clone());
        let _ = app.emit_to(LABEL, "receipt://show", receipt);
        match paints.recv_timeout(PAINT_DEADLINE) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => continue,
        }
        place(&app, &window);
        let _ = window.show();
        if p.sound && tone == Tone::Success {
            platform::play(sound(&p.sound_id), p.volume);
        }
        std::thread::sleep(VISIBLE_FOR);
        let _ = window.hide();
    }
}

/// Bottom-right corner of the work area on the monitor under the cursor.
fn place(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (w, h) = (
        snap(WIDTH + 2.0 * GUTTER, scale),
        snap(HEIGHT + 2.0 * GUTTER, scale),
    );
    let _ = window.set_size(PhysicalSize::new(w.round() as u32, h.round() as u32));
    let inset = (MARGIN - GUTTER) * scale;
    let x = area.position.x as f64 + area.size.width as f64 - w - inset;
    let y = area.position.y as f64 + area.size.height as f64 - h - inset;
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// Physical size of the smallest logical length >= `logical` that is a whole
/// number of physical pixels at `scale` (390 at 125 % gives 392 -> 490 px).
fn snap(logical: f64, scale: f64) -> f64 {
    (0..8)
        .map(|extra| (logical + extra as f64) * scale)
        .find(|px| (px - px.round()).abs() < 1e-6)
        .unwrap_or(logical * scale)
        .round()
}

/// Scales 16-bit PCM samples of a WAV in place. Other layouts pass through.
fn with_volume(wav: &[u8], volume: u8) -> Vec<u8> {
    let mut out = wav.to_vec();
    if volume >= 100 || wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return out;
    }
    let gain = (volume as f32 / 100.0).powi(2);
    let mut at = 12;
    let mut bits = 0u16;
    while at + 8 <= out.len() {
        let id = [out[at], out[at + 1], out[at + 2], out[at + 3]];
        let size =
            u32::from_le_bytes([out[at + 4], out[at + 5], out[at + 6], out[at + 7]]) as usize;
        let body = at + 8;
        if &id == b"fmt " && size >= 16 && body + 16 <= out.len() {
            bits = u16::from_le_bytes([out[body + 14], out[body + 15]]);
        }
        if &id == b"data" && bits == 16 {
            let end = (body + size).min(out.len());
            for s in out[body..end].chunks_exact_mut(2) {
                let v = i16::from_le_bytes([s[0], s[1]]) as f32 * gain;
                s.copy_from_slice(&(v as i16).to_le_bytes());
            }
            break;
        }
        at = body + size + (size & 1);
    }
    out
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    use windows_sys::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS};

    pub fn accepts_notifications() -> bool {
        let mut state = 0;
        // If Windows cannot tell, show the card.
        let failed = unsafe { SHQueryUserNotificationState(&mut state) } != 0;
        failed || state == QUNS_ACCEPTS_NOTIFICATIONS
    }

    pub fn play(wav: &'static [u8], volume: u8) {
        if volume == 0 {
            return;
        }
        let sound = super::with_volume(wav, volume);
        std::thread::spawn(move || unsafe {
            PlaySoundW(
                sound.as_ptr().cast(),
                std::ptr::null_mut(),
                SND_MEMORY | SND_SYNC | SND_NODEFAULT,
            );
        });
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn accepts_notifications() -> bool {
        true
    }
    pub fn play(_wav: &'static [u8], _volume: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(wav: &[u8]) -> Vec<i16> {
        let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        wav[data..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    #[test]
    fn window_sizes_are_whole_pixels_at_fractional_scales() {
        assert_eq!(snap(394.0, 1.0), 394.0);
        assert_eq!(snap(394.0, 1.25), 495.0); // 396 logical
        assert_eq!(snap(162.0, 1.25), 205.0); // 164 logical
        assert_eq!(snap(394.0, 1.5), 591.0);
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let px = snap(394.0, scale);
            assert!((px / scale).fract() < 1e-9, "{scale}: {px}");
        }
    }

    #[test]
    fn sounds_are_16_bit_wav() {
        for (_, wav) in SOUNDS {
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
        }
        assert_eq!(sound("nope"), SOUNDS[0].1);
    }

    #[test]
    fn volume_scales_samples_and_keeps_header() {
        let chime = SOUNDS[0].1;
        let full = samples(chime);
        let half = with_volume(chime, 50);
        assert_eq!(&half[..44], &chime[..44]);
        let quiet = samples(&half);
        let peak = |s: &[i16]| s.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak(&quiet) * 3 < peak(&full));
        assert_eq!(with_volume(chime, 100), chime);
        assert!(samples(&with_volume(chime, 0)).iter().all(|&v| v == 0));
    }
}
