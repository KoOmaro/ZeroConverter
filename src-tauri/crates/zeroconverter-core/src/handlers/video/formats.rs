//! Video family format table: the single source of truth for what can be read
//! and written. Audio targets are write-only here; their names belong to the
//! audio family when read.
use crate::conversion::types::FormatDescriptor;

const fn rw(id: &'static str, aliases: &'static [&'static str]) -> FormatDescriptor {
    FormatDescriptor {
        id,
        canonical_extension: id,
        aliases,
        can_read: true,
        can_write: true,
    }
}

const fn write_only(id: &'static str) -> FormatDescriptor {
    FormatDescriptor {
        id,
        canonical_extension: id,
        aliases: &[],
        can_read: false,
        can_write: true,
    }
}

pub static VIDEO_FORMATS: &[FormatDescriptor] = &[
    rw("mp4", &["m4v"]),
    rw("mkv", &[]),
    rw("mov", &[]),
    rw("webm", &[]),
    FormatDescriptor {
        id: "avi",
        canonical_extension: "avi",
        aliases: &[],
        can_read: true,
        can_write: false,
    },
    write_only("gif"),
    write_only("wav"),
    // Writing MP3 needs the LAME encoder (`mp3` feature).
    FormatDescriptor {
        id: "mp3",
        canonical_extension: "mp3",
        aliases: &[],
        can_read: false,
        can_write: cfg!(feature = "mp3"),
    },
    write_only("flac"),
    write_only("aiff"),
    write_only("opus"),
    write_only("m4a"),
    write_only("aac"),
];

/// What a target id produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Container(Container),
    Gif,
    Audio(AudioTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Mkv,
    Mov,
    Webm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTarget {
    Wav,
    Mp3,
    Flac,
    Aiff,
    Opus,
    M4a,
    Aac,
}

impl Container {
    /// FFmpeg muxer name, needed because outputs are written to a `.tmp` name.
    pub fn muxer(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "matroska",
            Container::Mov => "mov",
            Container::Webm => "webm",
        }
    }
}

pub fn target_of(format: &str) -> Option<Target> {
    Some(match format.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => Target::Container(Container::Mp4),
        "mkv" => Target::Container(Container::Mkv),
        "mov" => Target::Container(Container::Mov),
        "webm" => Target::Container(Container::Webm),
        "gif" => Target::Gif,
        "wav" => Target::Audio(AudioTarget::Wav),
        "mp3" => Target::Audio(AudioTarget::Mp3),
        "flac" => Target::Audio(AudioTarget::Flac),
        "aiff" => Target::Audio(AudioTarget::Aiff),
        "opus" => Target::Audio(AudioTarget::Opus),
        "m4a" => Target::Audio(AudioTarget::M4a),
        "aac" => Target::Audio(AudioTarget::Aac),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::types::find_descriptor;

    #[test]
    fn every_writable_descriptor_has_a_target() {
        for d in VIDEO_FORMATS.iter().filter(|d| d.can_write) {
            assert!(target_of(d.id).is_some(), "{} has no target", d.id);
        }
    }

    #[test]
    fn ids_are_unique_and_m4v_is_an_mp4_alias() {
        for (i, d) in VIDEO_FORMATS.iter().enumerate() {
            assert!(VIDEO_FORMATS[i + 1..].iter().all(|o| o.id != d.id));
        }
        assert_eq!(find_descriptor(VIDEO_FORMATS, "M4V").unwrap().id, "mp4");
    }

    #[test]
    fn avi_is_read_only() {
        let avi = find_descriptor(VIDEO_FORMATS, "avi").unwrap();
        assert!(avi.can_read && !avi.can_write);
    }
}
