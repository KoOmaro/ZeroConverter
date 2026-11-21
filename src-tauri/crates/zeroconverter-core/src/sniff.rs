// ==============================================================================
// OWNERSHIP:
//   Identifies what a file really is from its first bytes, independent of its
//   name. Used by rename-to-convert to tell "the user renamed a PNG to .webp"
//   apart from "this file already is a WebP".
//
// DO NOT ADD:
//   - Decoding or validation (handlers own that)
//   - Extension-based guesses
// ==============================================================================
use std::io::Read;
use std::path::Path;

/// Bytes needed to recognise every supported container.
pub const SNIFF_LEN: usize = 64;

/// Canonical format id of the bytes, or `None` when the content is unknown.
/// Ids match the converter's canonical format ids.
pub fn sniff(h: &[u8]) -> Option<&'static str> {
    let at = |off: usize, magic: &[u8]| {
        h.len() >= off + magic.len() && &h[off..off + magic.len()] == magic
    };

    if at(0, b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if at(0, b"\xFF\xD8\xFF") {
        return Some("jpg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("webp");
    }
    if at(0, b"RIFF") && at(8, b"WAVE") {
        return Some("wav");
    }
    if at(0, b"BM") && h.len() >= 14 {
        return Some("bmp");
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") {
        return Some("tiff");
    }
    if at(0, b"\0\0\x01\0") && h.len() >= 6 && h[4..6] != [0, 0] {
        return Some("ico");
    }
    if at(0, b"qoif") {
        return Some("qoi");
    }
    if at(0, b"8BPS") {
        return Some("psd");
    }
    if at(0, b"\xFF\x0A") || at(0, b"\0\0\0\x0CJXL \r\n\x87\n") {
        return Some("jxl");
    }
    if at(0, b"v/1\x01") {
        return Some("exr");
    }
    if at(0, b"#?RADIANCE") || at(0, b"#?RGBE") {
        return Some("hdr");
    }
    if at(0, b"DDS ") {
        return Some("dds");
    }
    if at(0, b"\xABKTX 20\xBB\r\n\x1A\n") {
        return Some("ktx2");
    }
    if at(0, b"%PDF-") {
        return Some("pdf");
    }
    if at(0, b"fLaC") {
        return Some("flac");
    }
    if at(0, b"OggS") {
        // The first page carries the codec header.
        let opus = h.windows(8).any(|w| w == b"OpusHead");
        return Some(if opus { "opus" } else { "ogg" });
    }
    if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        return Some("aiff");
    }
    if at(4, b"ftyp") {
        let brand = h.get(8..12).unwrap_or_default();
        return Some(match brand {
            b"avif" | b"avis" => "avif",
            b"M4A " | b"M4B " => "m4a",
            b"qt  " => "mov",
            _ => "mp4",
        });
    }
    if at(0, b"\x1A\x45\xDF\xA3") {
        // EBML header; its DocType says Matroska or WebM.
        let head = &h[..h.len().min(SNIFF_LEN)];
        return Some(if head.windows(4).any(|w| w == b"webm") {
            "webm"
        } else {
            "mkv"
        });
    }
    if at(0, b"RIFF") && at(8, b"AVI ") {
        return Some("avi");
    }
    if at(0, b"ID3") {
        return Some("mp3");
    }
    if h.len() >= 2 && h[0] == 0xFF {
        // ADTS AAC: sync + layer 00; MPEG audio (MP3): sync + layer != 00.
        if h[1] & 0xF6 == 0xF0 {
            return Some("aac");
        }
        if h[1] & 0xE0 == 0xE0 && h[1] & 0x06 != 0 {
            return Some("mp3");
        }
    }
    let text = String::from_utf8_lossy(&h[..h.len().min(SNIFF_LEN)]);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg")) {
        return Some("svg");
    }
    // Any other image format registered in its family's table (a new
    // format needs no line here).
    let head = &h[..h.len().min(crate::handlers::image::formats::SNIFF_LEN)];
    crate::handlers::image::formats::sniff(head).map(|m| m.descriptor.id)
}

/// Reads the head of a file and sniffs it. SVG files with a long XML prolog
/// get a wider look so the `<svg` root is still found.
pub fn sniff_file(path: &Path) -> Option<&'static str> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; 512];
    let n = file.read(&mut head).ok()?;
    head.truncate(n);
    sniff(&head).or_else(|| {
        let text = String::from_utf8_lossy(&head);
        text.contains("<svg").then_some("svg")
    })
}

/// Canonical id for an extension, folding the spellings people actually use.
pub fn canonical_ext(ext: &str) -> String {
    let e = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match e.as_str() {
        "jpeg" | "jpe" | "jfif" => "jpg".into(),
        "tif" => "tiff".into(),
        "aif" | "aifc" => "aiff".into(),
        "m4b" => "m4a".into(),
        "m4v" => "mp4".into(),
        "qt" => "mov".into(),
        "oga" => "ogg".into(),
        _ => e,
    }
}

/// True when content id and extension name the same format. MP4 and M4A are
/// the same container, so either name is honest for either.
pub fn same_format(content: &str, ext: &str) -> bool {
    let ext = canonical_ext(ext);
    content == ext || matches!((content, ext.as_str()), ("mp4", "m4a") | ("m4a", "mp4"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_images() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n0000"), Some("png"));
        assert_eq!(sniff(b"\xFF\xD8\xFF\xE0"), Some("jpg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"BM\0\0\0\0\0\0\0\0\0\0\0\0"), Some("bmp"));
        assert_eq!(sniff(b"II*\0"), Some("tiff"));
        assert_eq!(sniff(b"\0\0\x01\0\x01\0"), Some("ico"));
        assert_eq!(sniff(b"qoif"), Some("qoi"));
        assert_eq!(sniff(b"\0\0\0\x1cftypavif"), Some("avif"));
        assert_eq!(sniff(b"DDS |"), Some("dds"));
        assert_eq!(
            sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            Some("svg")
        );
    }

    /// Every image module that can write recognises its own output here, so
    /// rename-to-convert and the menu see the same formats as the queue.
    #[test]
    fn every_writable_image_module_is_recognised() {
        use crate::handlers::image::formats::MODULES;
        use crate::handlers::image::options::ImageOptions;
        let picture = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([40, 90, 200, 255]),
        ));
        for module in MODULES {
            let Some(encode) = module.encode else {
                continue;
            };
            let bytes = encode(&picture, &ImageOptions::default()).unwrap();
            assert_eq!(
                sniff(&bytes),
                Some(module.descriptor.id),
                "{} output is not recognised",
                module.descriptor.id
            );
        }
    }

    #[test]
    fn sniffs_audio_and_video() {
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt "), Some("wav"));
        assert_eq!(sniff(b"ID3\x04"), Some("mp3"));
        assert_eq!(sniff(b"\xFF\xFB\x90\x00"), Some("mp3"));
        assert_eq!(sniff(b"\xFF\xF1\x50\x80"), Some("aac"));
        assert_eq!(sniff(b"fLaC"), Some("flac"));
        assert_eq!(
            sniff(b"OggS\0\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01\x13OpusHead"),
            Some("opus")
        );
        assert_eq!(
            sniff(b"OggS\0\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01\x1e\x01vorbis"),
            Some("ogg")
        );
        assert_eq!(sniff(b"FORM\0\0\0\0AIFF"), Some("aiff"));
        assert_eq!(sniff(b"\0\0\0\x20ftypisom"), Some("mp4"));
        assert_eq!(sniff(b"\0\0\0\x20ftypM4A "), Some("m4a"));
        assert_eq!(sniff(b"\0\0\0\x14ftypqt  \0\0\0\0qt  "), Some("mov"));
        assert_eq!(
            sniff(b"\x1A\x45\xDF\xA3\x9F\x42\x82\x88matroska"),
            Some("mkv")
        );
        assert_eq!(sniff(b"\x1A\x45\xDF\xA3\x9F\x42\x82\x84webm"), Some("webm"));
        assert_eq!(sniff(b"RIFF\0\0\0\0AVI LIST"), Some("avi"));
    }

    #[test]
    fn unknown_content_is_none() {
        assert_eq!(sniff(b"hello world"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn extension_spellings_fold() {
        assert_eq!(canonical_ext(".JPEG"), "jpg");
        assert_eq!(canonical_ext("tif"), "tiff");
        assert!(same_format("jpg", "jpeg"));
        assert!(same_format("mp4", "m4a"));
        assert!(!same_format("png", "webp"));
    }
}
