// ==============================================================================
// OWNERSHIP:
//   Decides whether a rename the user made means "convert this file".
//   Pure: no file system access beyond what the caller passes in.
// ==============================================================================
use crate::core::sniff::{canonical_ext, same_format};
use std::path::{Path, PathBuf};

/// Trailing character that asks for the options window: `photo.webp+`.
pub const OPTIONS_SUFFIX: char = '+';
/// Keep the original next to the converted file: `photo.webp&`.
pub const KEEP_SUFFIX: char = '&';
/// Shrink so the longest side is at most N pixels: `photo.webp@1080`.
pub const SIZE_MARK: char = '@';
const SIZE_RANGE: std::ops::RangeInclusive<u32> = 16..=16_384;

/// What the user added after the extension. Any order, each at most once:
/// `photo.jpg&@1080+`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ask_options: bool,
    pub keep_original: bool,
    pub max_size: Option<u32>,
}

impl Modifiers {
    pub fn any(&self) -> bool {
        self.ask_options || self.keep_original || self.max_size.is_some()
    }
}

/// `photo.jpg&@1080+` → (`photo.jpg`, keep + 1080 px + options window).
pub fn split_modifiers(name: &str) -> (&str, Modifiers) {
    let mut m = Modifiers::default();
    let mut rest = name;
    loop {
        if !m.ask_options && rest.ends_with(OPTIONS_SUFFIX) {
            m.ask_options = true;
            rest = &rest[..rest.len() - 1];
        } else if !m.keep_original && rest.ends_with(KEEP_SUFFIX) {
            m.keep_original = true;
            rest = &rest[..rest.len() - 1];
        } else if let Some((head, size)) = m
            .max_size
            .is_none()
            .then(|| rest.rsplit_once(SIZE_MARK))
            .flatten()
            .filter(|(_, n)| !n.is_empty() && n.len() <= 5 && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|(head, n)| n.parse::<u32>().ok().map(|n| (head, n)))
            .filter(|(_, n)| SIZE_RANGE.contains(n))
        {
            m.max_size = Some(size);
            rest = head;
        } else {
            return (rest, m);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Not ours: leave the file alone.
    Ignore,
    /// Convert `current` (whose bytes are `source`) into `target`.
    Convert {
        source: String,
        target: String,
        modifiers: Modifiers,
        /// Where the converted file ends up (`+`, `&`, `@N` stripped).
        final_path: PathBuf,
    },
    /// The user asked for a format ZeroConverter cannot produce from this file.
    Unsupported { source: String, target: String },
    /// Only marks were added and there is nothing to do (`photo.png&`): put
    /// the old name back.
    Restore,
}

/// A folder renamed by adding a format: `Holiday` → `Holiday.webp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderConvert {
    pub target: String,
    pub modifiers: Modifiers,
}

/// `old` and `new` are the folder's paths before and after the rename.
/// `writable` says whether ZeroConverter can write a format id.
pub fn decide_folder(
    old: &Path,
    new: &Path,
    writable: impl Fn(&str) -> bool,
) -> Option<FolderConvert> {
    if old.parent() != new.parent() {
        return None;
    }
    let old_name = old.file_name()?.to_str()?;
    let (clean, modifiers) = split_modifiers(new.file_name()?.to_str()?);
    let ext = clean.strip_prefix(old_name)?.strip_prefix('.')?;
    let target = canonical_ext(&ext.to_ascii_lowercase());
    writable(&target).then_some(FolderConvert { target, modifiers })
}

/// `old` and `new` are full paths of one rename event. `content` is the
/// sniffed format of the file now at `new`. `known` says whether an id is a
/// format ZeroConverter reads or writes; `can_route(source, target)` whether
/// it can convert between them.
pub fn decide(
    old: &Path,
    new: &Path,
    content: Option<&str>,
    known: impl Fn(&str) -> bool,
    can_route: impl Fn(&str, &str) -> bool,
) -> Decision {
    if old.parent() != new.parent() {
        return Decision::Ignore;
    }
    let Some(new_name) = new.file_name().and_then(|n| n.to_str()) else {
        return Decision::Ignore;
    };
    if is_transient_name(new_name) {
        return Decision::Ignore;
    }
    let (clean_name, modifiers) = split_modifiers(new_name);
    let clean = new.with_file_name(clean_name);

    let (Some(old_ext), Some(new_ext)) = (ext_of(old), ext_of(&clean)) else {
        return Decision::Ignore;
    };
    let (old_ext, new_ext) = (canonical_ext(&old_ext), canonical_ext(&new_ext));
    if !known(&old_ext) || !known(&new_ext) {
        return Decision::Ignore;
    }
    // Only honest renames: the bytes must be what the old name said.
    let Some(content) = content else {
        return Decision::Ignore;
    };
    if !same_format(content, &old_ext) {
        return Decision::Ignore;
    }
    if same_format(content, &new_ext) {
        // `photo.jpeg` → `photo.jpg` is a spelling change. Same format with
        // `+` or `@N` re-encodes (options, a smaller copy).
        return if modifiers.ask_options || modifiers.max_size.is_some() {
            Decision::Convert {
                source: content.to_string(),
                target: new_ext,
                modifiers,
                final_path: clean,
            }
        } else if modifiers.any() {
            Decision::Restore
        } else {
            Decision::Ignore
        };
    }
    if !can_route(content, &new_ext) {
        return Decision::Unsupported {
            source: content.to_string(),
            target: new_ext,
        };
    }
    Decision::Convert {
        source: content.to_string(),
        target: new_ext,
        modifiers,
        final_path: clean,
    }
}

fn ext_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(str::to_string)
}

/// Names written by editors, downloaders and ZeroConverter itself.
pub fn is_transient_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("~$")
        || lower.starts_with(".zc-")
        || lower.contains(".zc-src-")
        || lower.contains(".zc-out-")
        || lower.ends_with(".tmp")
        || lower.ends_with(".crdownload")
        || lower.ends_with(".part")
        || lower.ends_with(".partial")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &[
        "png", "jpg", "webp", "gif", "mp3", "wav", "mp4", "m4a", "flac",
    ];
    fn known(id: &str) -> bool {
        KNOWN.contains(&id)
    }
    fn route(src: &str, dst: &str) -> bool {
        let image = ["png", "jpg", "webp", "gif"];
        let audio = ["mp3", "wav", "mp4", "m4a", "flac"];
        let writable = ["png", "jpg", "webp", "mp3", "wav", "flac"];
        writable.contains(&dst)
            && ((image.contains(&src) && image.contains(&dst))
                || (audio.contains(&src) && audio.contains(&dst)))
    }
    fn d(old: &str, new: &str, content: Option<&str>) -> Decision {
        decide(Path::new(old), Path::new(new), content, known, route)
    }

    #[test]
    fn renaming_png_to_webp_converts() {
        assert_eq!(
            d("C:/a/photo.png", "C:/a/photo.webp", Some("png")),
            Decision::Convert {
                source: "png".into(),
                target: "webp".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/a/photo.webp".into()
            }
        );
    }

    #[test]
    fn mp4_to_mp3_converts() {
        assert!(matches!(
            d("C:/v/clip.mp4", "C:/v/clip.mp3", Some("mp4")),
            Decision::Convert { .. }
        ));
    }

    #[test]
    fn plus_suffix_asks_for_options_and_is_stripped() {
        match d("C:/a/photo.png", "C:/a/photo.webp+", Some("png")) {
            Decision::Convert {
                modifiers,
                final_path,
                ..
            } => {
                assert!(modifiers.ask_options);
                assert_eq!(final_path, Path::new("C:/a/photo.webp"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn marks_combine_in_any_order() {
        let full = Modifiers {
            ask_options: true,
            keep_original: true,
            max_size: Some(1080),
        };
        for name in [
            "a.jpg&@1080+",
            "a.jpg+&@1080",
            "a.jpg@1080&+",
            "a.jpg@1080+&",
        ] {
            assert_eq!(split_modifiers(name), ("a.jpg", full), "{name}");
        }
        assert_eq!(split_modifiers("a.jpg"), ("a.jpg", Modifiers::default()));
        // Out of range or not a number: not a size mark.
        assert_eq!(split_modifiers("a.jpg@8").1.max_size, None);
        assert_eq!(split_modifiers("a.jpg@99999").1.max_size, None);
        assert_eq!(
            split_modifiers("me@home.jpg"),
            ("me@home.jpg", Modifiers::default())
        );
    }

    #[test]
    fn keep_and_size_marks_convert_to_the_clean_name() {
        match d("C:/a/photo.png", "C:/a/photo.webp&@512", Some("png")) {
            Decision::Convert {
                modifiers,
                final_path,
                ..
            } => {
                assert!(modifiers.keep_original);
                assert_eq!(modifiers.max_size, Some(512));
                assert_eq!(final_path, Path::new("C:/a/photo.webp"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn size_mark_on_the_same_format_makes_a_smaller_copy() {
        assert!(matches!(
            d("C:/a/photo.png", "C:/a/photo.png&@256", Some("png")),
            Decision::Convert { .. }
        ));
        assert_eq!(
            d("C:/a/photo.png", "C:/a/photo.png&", Some("png")),
            Decision::Restore
        );
    }

    #[test]
    fn folder_renamed_with_a_format_converts_its_files() {
        let w = |id: &str| ["webp", "jpg", "png"].contains(&id);
        assert_eq!(
            decide_folder(
                Path::new("C:/a/Holiday"),
                Path::new("C:/a/Holiday.webp&"),
                w
            ),
            Some(FolderConvert {
                target: "webp".into(),
                modifiers: Modifiers {
                    keep_original: true,
                    ..Default::default()
                }
            })
        );
        assert_eq!(
            decide_folder(Path::new("C:/a/v1.2"), Path::new("C:/a/v1.2.JPEG"), w).map(|f| f.target),
            Some("jpg".into())
        );
        // Renamed to something else, or a format it cannot write.
        assert_eq!(
            decide_folder(Path::new("C:/a/Holiday"), Path::new("C:/a/Trip.webp"), w),
            None
        );
        assert_eq!(
            decide_folder(Path::new("C:/a/Holiday"), Path::new("C:/a/Holiday.zip"), w),
            None
        );
        assert_eq!(
            decide_folder(Path::new("C:/a/Holiday"), Path::new("C:/b/Holiday.webp"), w),
            None
        );
    }

    #[test]
    fn spelling_change_is_ignored() {
        assert_eq!(
            d("C:/a/x.jpeg", "C:/a/x.jpg", Some("jpg")),
            Decision::Ignore
        );
    }

    #[test]
    fn content_must_match_old_name() {
        // A WebP that was misnamed .png and is now fixed: not a conversion.
        assert_eq!(
            d("C:/a/x.png", "C:/a/x.webp", Some("webp")),
            Decision::Ignore
        );
        assert_eq!(d("C:/a/x.png", "C:/a/x.webp", None), Decision::Ignore);
    }

    #[test]
    fn unknown_extensions_and_downloads_are_ignored() {
        assert_eq!(
            d("C:/a/x.png", "C:/a/x.png.bak", Some("png")),
            Decision::Ignore
        );
        assert_eq!(
            d("C:/a/x.crdownload", "C:/a/x.png", Some("png")),
            Decision::Ignore
        );
        assert_eq!(
            d("C:/a/x.png", "C:/a/~$x.webp", Some("png")),
            Decision::Ignore
        );
    }

    #[test]
    fn moves_between_folders_are_ignored() {
        assert_eq!(
            d("C:/a/x.png", "C:/b/x.webp", Some("png")),
            Decision::Ignore
        );
    }

    #[test]
    fn impossible_route_is_reported() {
        assert_eq!(
            d("C:/a/song.mp3", "C:/a/song.png", Some("mp3")),
            Decision::Unsupported {
                source: "mp3".into(),
                target: "png".into()
            }
        );
        assert_eq!(
            d("C:/a/x.png", "C:/a/x.gif", Some("png")),
            Decision::Unsupported {
                source: "png".into(),
                target: "gif".into()
            }
        );
    }
}
