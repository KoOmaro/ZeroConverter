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

/// What reading and sniffing the file at `new` produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content<'a> {
    /// The file could not be opened or read (locked, cloud placeholder, I/O error).
    Unreadable,
    /// The file was read, but its bytes matched no format ZeroConverter knows.
    Unknown,
    /// The file was read and sniffed as this canonical format id.
    Known(&'a str),
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
    /// The user meant a conversion, but the file could not be read: put the
    /// old name back and explain.
    Unreadable,
    /// The user meant a conversion, but the file's bytes do not match its old
    /// extension (`expected`): put the old name back and explain.
    Mismatch {
        expected: String,
        actual: Option<String>,
    },
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

/// True when `decide(old, new, ...)` could return anything other than
/// [`Decision::Ignore`]. Lets the watcher skip reading/retrying files on
/// ordinary same-extension renames, moves between folders, or unknown formats.
pub fn may_need_sniff(old: &Path, new: &Path, known: impl Fn(&str) -> bool) -> bool {
    if old.parent() != new.parent() {
        return false;
    }
    let Some(new_name) = new.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if is_transient_name(new_name) {
        return false;
    }
    let (clean_name, modifiers) = split_modifiers(new_name);
    let clean = new.with_file_name(clean_name);
    let (Some(old_ext), Some(new_ext)) = (ext_of(old), ext_of(&clean)) else {
        return false;
    };
    let (old_ext, new_ext) = (canonical_ext(&old_ext), canonical_ext(&new_ext));
    if !known(&old_ext) || !known(&new_ext) {
        return false;
    }
    !same_format(&old_ext, &new_ext)
        || is_mp4_to_m4a(&old_ext, &new_ext)
        || modifiers.any()
        || parse_hidden_ext(old, new, new_name, &known).is_some()
}

/// `mp4` (or `.m4v`) renamed to `.m4a` extracts the audio track even though
/// both use ISO BMFF (`same_format("mp4", "m4a")` is true for sniffing).
fn is_mp4_to_m4a(old_ext: &str, new_ext: &str) -> bool {
    old_ext == "mp4" && new_ext == "m4a"
}

/// `old` and `new` are full paths of one rename event. `content` is what
/// reading and sniffing the file now at `new` produced. `known` says whether
/// an id is a format ZeroConverter reads or writes; `can_route(source, target)`
/// whether it can convert between them.
pub fn decide(
    old: &Path,
    new: &Path,
    content: Content<'_>,
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
    let hidden = (!modifiers.any())
        .then(|| parse_hidden_ext(old, new, new_name, &known))
        .flatten();
    let hidden_marked = hidden.as_ref().is_some_and(|h| h.modifiers.any());
    let mp4_audio = is_mp4_to_m4a(&old_ext, &new_ext);
    let meant_conversion = ((!same_format(&old_ext, &new_ext) || mp4_audio)
        && same_stem(old, &clean))
        || hidden_marked;
    let content = match content {
        Content::Unreadable => {
            return if meant_conversion {
                Decision::Unreadable
            } else {
                Decision::Ignore
            };
        }
        Content::Unknown => {
            return if meant_conversion {
                Decision::Mismatch {
                    expected: old_ext,
                    actual: None,
                }
            } else {
                Decision::Ignore
            };
        }
        Content::Known(c) => c,
    };
    // Only honest renames: the bytes must be what the old name said.
    if !same_format(content, &old_ext) {
        // Renaming a misnamed file to its real extension without conversion marks
        // fixes the name and stays untouched; any other target explains why.
        if meant_conversion && (hidden_marked || !same_format(content, &new_ext) || modifiers.any())
        {
            return Decision::Mismatch {
                expected: old_ext,
                actual: Some(content.to_string()),
            };
        }
        return Decision::Ignore;
    }
    if same_format(content, &new_ext) && !mp4_audio {
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
            hidden
                .and_then(|h| finish_hidden_ext(h, content, &can_route))
                .unwrap_or(Decision::Ignore)
        };
    }
    let route_source = if mp4_audio { "mp4" } else { content };
    if !can_route(route_source, &new_ext) {
        return Decision::Unsupported {
            source: route_source.to_string(),
            target: new_ext,
        };
    }
    Decision::Convert {
        source: route_source.to_string(),
        target: new_ext,
        modifiers,
        final_path: clean,
    }
}

fn same_stem(a: &Path, b: &Path) -> bool {
    let (Some(sa), Some(sb)) = (
        a.file_stem().and_then(|s| s.to_str()),
        b.file_stem().and_then(|s| s.to_str()),
    ) else {
        return false;
    };
    !sa.is_empty() && sa.to_lowercase() == sb.to_lowercase()
}

struct HiddenExt {
    old_ext: String,
    target: String,
    modifiers: Modifiers,
    final_path: PathBuf,
}

/// Explorer with "File name extensions" off turns `song` (really `song.m4a`)
/// renamed to `song.mp3` (or `song.mp3&.m4a`) into `song.mp3.m4a`. Parses
/// `<stem>.<target>[modifiers].<ext>` when stem and trailing ext match `old`
/// (case-insensitive) and `<target>` is a known format.
fn parse_hidden_ext(
    old: &Path,
    new: &Path,
    new_name: &str,
    known: &impl Fn(&str) -> bool,
) -> Option<HiddenExt> {
    let old_name = old.file_name()?.to_str()?;
    let (old_stem, old_ext_raw) = old_name.rsplit_once('.')?;
    let (new_body, new_ext_raw) = new_name.rsplit_once('.')?;
    if old_stem.is_empty()
        || old_ext_raw.is_empty()
        || !old_ext_raw.eq_ignore_ascii_case(new_ext_raw)
    {
        return None;
    }
    let (clean_body, modifiers) = split_modifiers(new_body);
    let (new_stem, target_raw) = clean_body.rsplit_once('.')?;
    if new_stem.is_empty()
        || target_raw.is_empty()
        || !target_raw.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    if new_stem.to_lowercase() != old_stem.to_lowercase() {
        return None;
    }
    let target = canonical_ext(target_raw);
    if !known(&target) {
        return None;
    }
    Some(HiddenExt {
        old_ext: canonical_ext(old_ext_raw),
        target,
        modifiers,
        final_path: new.with_file_name(clean_body),
    })
}

fn finish_hidden_ext(
    h: HiddenExt,
    content: &str,
    can_route: &impl Fn(&str, &str) -> bool,
) -> Option<Decision> {
    let mp4_audio = is_mp4_to_m4a(&h.old_ext, &h.target);
    if same_format(content, &h.target) && !mp4_audio {
        return if h.modifiers.ask_options || h.modifiers.max_size.is_some() {
            Some(Decision::Convert {
                source: content.to_string(),
                target: h.target,
                modifiers: h.modifiers,
                final_path: h.final_path,
            })
        } else if h.modifiers.any() {
            Some(Decision::Restore)
        } else {
            None
        };
    }
    let route_source = if mp4_audio { "mp4" } else { content };
    if !can_route(route_source, &h.target) {
        return h.modifiers.any().then(|| Decision::Unsupported {
            source: route_source.to_string(),
            target: h.target,
        });
    }
    Some(Decision::Convert {
        source: route_source.to_string(),
        target: h.target,
        modifiers: h.modifiers,
        final_path: h.final_path,
    })
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
        (src == "mp4" && dst == "m4a")
            || (writable.contains(&dst)
                && ((image.contains(&src) && image.contains(&dst))
                    || (audio.contains(&src) && audio.contains(&dst))))
    }
    fn d(old: &str, new: &str, content: Content<'_>) -> Decision {
        decide(Path::new(old), Path::new(new), content, known, route)
    }

    fn d_ok(old: &str, new: &str, content: &str) -> Decision {
        d(old, new, Content::Known(content))
    }

    #[test]
    fn renaming_png_to_webp_converts() {
        assert_eq!(
            d_ok("C:/a/photo.png", "C:/a/photo.webp", "png"),
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
            d_ok("C:/v/clip.mp4", "C:/v/clip.mp3", "mp4"),
            Decision::Convert { .. }
        ));
    }

    #[test]
    fn plus_suffix_asks_for_options_and_is_stripped() {
        match d_ok("C:/a/photo.png", "C:/a/photo.webp+", "png") {
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
        match d_ok("C:/a/photo.png", "C:/a/photo.webp&@512", "png") {
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
            d_ok("C:/a/photo.png", "C:/a/photo.png&@256", "png"),
            Decision::Convert { .. }
        ));
        assert_eq!(
            d_ok("C:/a/photo.png", "C:/a/photo.png&", "png"),
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
        assert_eq!(d_ok("C:/a/x.jpeg", "C:/a/x.jpg", "jpg"), Decision::Ignore);
    }

    #[test]
    fn fixing_misnamed_file_to_its_actual_format_is_ignored() {
        // A WebP that was misnamed .png and is now fixed: not a conversion.
        assert_eq!(d_ok("C:/a/x.png", "C:/a/x.webp", "webp"), Decision::Ignore);
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3", "mp3"),
            Decision::Ignore
        );
    }

    #[test]
    fn unreadable_conversion_is_explained() {
        assert_eq!(
            d("C:/m/song.m4a", "C:/m/song.mp3", Content::Unreadable),
            Decision::Unreadable
        );
        assert_eq!(
            d("C:/m/My.Song.M4A", "C:/m/my.song.mp3&", Content::Unreadable),
            Decision::Unreadable
        );
    }

    #[test]
    fn mismatched_content_is_explained() {
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3", "webm"),
            Decision::Mismatch {
                expected: "m4a".into(),
                actual: Some("webm".into()),
            }
        );
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.wav", "mp3"),
            Decision::Mismatch {
                expected: "m4a".into(),
                actual: Some("mp3".into()),
            }
        );
        assert_eq!(
            d("C:/m/song.m4a", "C:/m/song.mp3", Content::Unknown),
            Decision::Mismatch {
                expected: "m4a".into(),
                actual: None,
            }
        );
        // Conversion marks mean convert, not fixing a misnamed extension.
        assert_eq!(
            d_ok("C:/a/x.png", "C:/a/x.webp+", "webp"),
            Decision::Mismatch {
                expected: "png".into(),
                actual: Some("webp".into()),
            }
        );
    }

    #[test]
    fn non_conversion_renames_stay_silent_even_when_unreadable_or_mismatched() {
        for content in [Content::Unreadable, Content::Unknown, Content::Known("wav")] {
            // Transient names.
            assert_eq!(d("C:/a/x.png", "C:/a/~$x.webp", content), Decision::Ignore);
            assert_eq!(
                d("C:/a/x.crdownload", "C:/a/x.png", content),
                Decision::Ignore
            );
            // Unknown extensions.
            assert_eq!(d("C:/a/x.bak", "C:/a/x.png", content), Decision::Ignore);
            assert_eq!(d("C:/a/x.png", "C:/a/x.bak", content), Decision::Ignore);
            // Moves between folders.
            assert_eq!(d("C:/a/x.png", "C:/b/x.webp", content), Decision::Ignore);
            // Spelling changes / same format.
            assert_eq!(d("C:/a/x.jpeg", "C:/a/x.jpg", content), Decision::Ignore);
            assert_eq!(d("C:/a/x.png", "C:/a/x.PNG", content), Decision::Ignore);
            // Stem changed.
            assert_eq!(d("C:/a/x.png", "C:/a/y.webp", content), Decision::Ignore);
            // Hidden-extension pattern.
            assert_eq!(
                d("C:/m/song.m4a", "C:/m/song.mp3.m4a", content),
                Decision::Ignore
            );
        }
    }

    #[test]
    fn unknown_extensions_and_downloads_are_ignored() {
        assert_eq!(
            d_ok("C:/a/x.png", "C:/a/x.png.bak", "png"),
            Decision::Ignore
        );
        assert_eq!(
            d_ok("C:/a/x.crdownload", "C:/a/x.png", "png"),
            Decision::Ignore
        );
        assert_eq!(d_ok("C:/a/x.png", "C:/a/~$x.webp", "png"), Decision::Ignore);
    }

    #[test]
    fn moves_between_folders_are_ignored() {
        assert_eq!(d_ok("C:/a/x.png", "C:/b/x.webp", "png"), Decision::Ignore);
    }

    #[test]
    fn impossible_route_is_reported() {
        assert_eq!(
            d_ok("C:/a/song.mp3", "C:/a/song.png", "mp3"),
            Decision::Unsupported {
                source: "mp3".into(),
                target: "png".into()
            }
        );
        assert_eq!(
            d_ok("C:/a/x.png", "C:/a/x.gif", "png"),
            Decision::Unsupported {
                source: "png".into(),
                target: "gif".into()
            }
        );
    }

    #[test]
    fn hidden_extension_m4a_to_mp3_converts_without_trailing_ext() {
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3.m4a", "m4a"),
            Decision::Convert {
                source: "m4a".into(),
                target: "mp3".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/m/song.mp3".into(),
            }
        );
        // `.m4a` files with an `isom` brand sniff as `mp4`.
        assert_eq!(
            d_ok("C:/m/Song.M4A", "C:/m/song.MP3.m4a", "mp4"),
            Decision::Convert {
                source: "mp4".into(),
                target: "mp3".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/m/song.MP3".into(),
            }
        );
    }

    #[test]
    fn hidden_extension_image_converts_and_preserves_dotted_stem() {
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.jpg.png", "png"),
            Decision::Convert {
                source: "png".into(),
                target: "jpg".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/p/photo.jpg".into(),
            }
        );
        assert_eq!(
            d_ok("C:/p/my.photo.png", "C:/p/my.photo.jpeg.png", "png"),
            Decision::Convert {
                source: "png".into(),
                target: "jpg".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/p/my.photo.jpeg".into(),
            }
        );
    }

    #[test]
    fn hidden_extension_supports_modifiers_before_hidden_ext() {
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3&.m4a", "m4a"),
            Decision::Convert {
                source: "m4a".into(),
                target: "mp3".into(),
                modifiers: Modifiers {
                    keep_original: true,
                    ..Default::default()
                },
                final_path: "C:/m/song.mp3".into(),
            }
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.webp&@512+.png", "png"),
            Decision::Convert {
                source: "png".into(),
                target: "webp".into(),
                modifiers: Modifiers {
                    ask_options: true,
                    keep_original: true,
                    max_size: Some(512),
                },
                final_path: "C:/p/photo.webp".into(),
            }
        );
    }

    #[test]
    fn hidden_extension_rejects_mismatched_stem_trailing_ext_or_unsupported_middle_ext() {
        // Mismatched stem.
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/other.mp3.m4a", "m4a"),
            Decision::Ignore
        );
        // Mismatched trailing ext.
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3.bak", "m4a"),
            Decision::Ignore
        );
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp3.mp4", "m4a"),
            Decision::Ignore
        );
        assert_eq!(
            d("C:/a/a.gz", "C:/a/a.tar.gz", Content::Unreadable),
            Decision::Ignore
        );
        // Unsupported or unknown middle ext.
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.png.m4a", "m4a"),
            Decision::Ignore
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.gif.png", "png"),
            Decision::Ignore
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.bak.png", "png"),
            Decision::Ignore
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.png.png", "png"),
            Decision::Ignore
        );
    }

    #[test]
    fn hidden_extension_leaves_normal_renames_unchanged() {
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.webp", "png"),
            Decision::Convert {
                source: "png".into(),
                target: "webp".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/p/photo.webp".into(),
            }
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.webp.jpg", "png"),
            Decision::Convert {
                source: "png".into(),
                target: "jpg".into(),
                modifiers: Modifiers::default(),
                final_path: "C:/p/photo.webp.jpg".into(),
            }
        );
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.png", "m4a"),
            Decision::Unsupported {
                source: "m4a".into(),
                target: "png".into(),
            }
        );
    }

    #[test]
    fn mp4_to_m4a_extracts_audio_while_reverse_stays_silent() {
        for brand in ["mp4", "m4a"] {
            assert_eq!(
                d_ok("C:/v/clip.mp4", "C:/v/clip.m4a", brand),
                Decision::Convert {
                    source: "mp4".into(),
                    target: "m4a".into(),
                    modifiers: Modifiers::default(),
                    final_path: "C:/v/clip.m4a".into(),
                }
            );
            assert_eq!(
                d_ok("C:/v/clip.mp4", "C:/v/clip.m4a.mp4", brand),
                Decision::Convert {
                    source: "mp4".into(),
                    target: "m4a".into(),
                    modifiers: Modifiers::default(),
                    final_path: "C:/v/clip.m4a".into(),
                }
            );
        }
        assert_eq!(
            d("C:/v/clip.mp4", "C:/v/clip.m4a", Content::Unreadable),
            Decision::Unreadable
        );
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.mp4", "mp4"),
            Decision::Ignore
        );
    }

    #[test]
    fn marked_hidden_extension_explains_errors_and_restores_keep_only() {
        assert_eq!(
            d(
                "C:/p/photo.png",
                "C:/p/photo.webp+.png",
                Content::Unreadable
            ),
            Decision::Unreadable
        );
        assert_eq!(
            d_ok("C:/m/song.m4a", "C:/m/song.png+.m4a", "m4a"),
            Decision::Unsupported {
                source: "m4a".into(),
                target: "png".into(),
            }
        );
        assert_eq!(
            d_ok("C:/p/photo.png", "C:/p/photo.png&.png", "png"),
            Decision::Restore
        );
    }

    #[test]
    fn may_need_sniff_skips_plain_same_extension_and_keeps_candidates() {
        let check = |a: &str, b: &str| may_need_sniff(Path::new(a), Path::new(b), known);
        assert!(!check("C:/a/one.png", "C:/a/two.png"));
        assert!(!check("C:/a/one.jpeg", "C:/a/one.jpg"));
        assert!(!check("C:/a/one.png", "C:/b/one.webp"));
        assert!(!check("C:/a/one.png", "C:/a/~$one.webp"));
        assert!(!check("C:/a/one.bak", "C:/a/one.png"));

        assert!(check("C:/a/one.png", "C:/a/one.webp"));
        assert!(check("C:/a/one.png", "C:/a/two.webp"));
        assert!(check("C:/a/one.png", "C:/a/one.png@512"));
        assert!(check("C:/a/one.png", "C:/a/one.webp.png"));
        assert!(check("C:/v/clip.mp4", "C:/v/clip.m4a"));
    }
}
