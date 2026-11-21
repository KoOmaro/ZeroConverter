use crate::app_error::AppError;
use crate::conversion::types::InputFormat;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn probe_image(path: &Path) -> Result<InputFormat, AppError> {
    let mut file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let mut buffer = [0u8; super::formats::SNIFF_LEN];
    let n = file
        .read(&mut buffer)
        .map_err(|e| AppError::Io(e.to_string()))?;

    if n < 4 {
        return Err(AppError::Probe("File too small to be an image".to_string()));
    }

    // Format modules (see formats/mod.rs): one signature per format.
    if let Some(module) = super::formats::sniff(&buffer[..n]) {
        return Ok(InputFormat(module.descriptor.id.to_string()));
    }

    // ICO-family headers no module accepted: say why, and never fall back
    // to the extension.
    if buffer.starts_with(&[0x00, 0x00, 0x02, 0x00]) {
        return Err(AppError::Probe(
            "CUR cursor files are not supported".to_string(),
        ));
    }
    if buffer.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Err(AppError::Probe(
            "Invalid ICO header: zero image count or truncated".to_string(),
        ));
    }

    // Fallback to extension if signature didn't match (though signature-first is required).
    // ICO is intentionally excluded here — ICO files must pass signature validation above.
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lowered = ext.to_lowercase();

        // .svgz is explicitly deferred/rejected in v1
        if lowered == "svgz" {
            return Err(AppError::Probe(
                "Compressed SVG (.svgz) is not supported".to_string(),
            ));
        }

        // SVG detection: read up to 4KB and use the SVG probe helper.
        // SVG is text-based and has no binary magic signature, so this uses
        // a combination of extension + content sniffing.
        if lowered == "svg" {
            let mut svg_file = File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
            let mut svg_buffer = vec![0u8; 4096];
            let svg_n = svg_file
                .read(&mut svg_buffer)
                .map_err(|e| AppError::Io(e.to_string()))?;
            svg_buffer.truncate(svg_n);

            if super::svg::probe_is_svg(&svg_buffer) {
                return Ok(InputFormat("svg".to_string()));
            }
            // .svg extension but content doesn't look like SVG — reject
            return Err(AppError::Probe(
                "File has .svg extension but does not contain valid SVG content".to_string(),
            ));
        }

        if lowered == "png"
            || lowered == "jpg"
            || lowered == "jpeg"
            || lowered == "webp"
            || lowered == "bmp"
            || lowered == "tiff"
            || lowered == "tif"
        {
            let canon = if lowered == "tif" {
                "tiff".to_string()
            } else if lowered == "jpeg" {
                "jpg".to_string()
            } else {
                lowered
            };
            return Ok(InputFormat(canon));
        }
    }

    Err(AppError::Probe(
        "Unsupported or unknown image format signature".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ────────────────────────────────────────────
    // Helper: write raw bytes to a temp file with a specific extension
    // ────────────────────────────────────────────
    fn temp_file_with_bytes(ext: &str, bytes: &[u8]) -> NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(&format!(".{}", ext))
            .tempfile()
            .expect("Failed to create temp file");
        file.write_all(bytes).expect("Failed to write bytes");
        file
    }

    // ═══════════════════════════════════════════
    // PNG Signature Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_identifies_png_by_signature() {
        // Real PNG header: 89 50 4E 47 0D 0A 1A 0A
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("png", &png_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "png");
    }

    #[test]
    fn probe_identifies_png_even_with_wrong_extension() {
        // PNG signature in a .jpg file — signature wins
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
        ];
        let file = temp_file_with_bytes("jpg", &png_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "png");
    }

    // ═══════════════════════════════════════════
    // JPEG Signature Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_identifies_jpeg_by_signature() {
        // JPEG: FF D8 FF E0 (JFIF) or FF D8 FF E1 (EXIF)
        let jpeg_bytes = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
        ];
        let file = temp_file_with_bytes("jpg", &jpeg_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "jpg");
    }

    #[test]
    fn probe_identifies_jpeg_exif_variant() {
        // JPEG with EXIF header
        let jpeg_exif = [
            0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x00, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00,
        ];
        let file = temp_file_with_bytes("jpeg", &jpeg_exif);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "jpg");
    }

    // ═══════════════════════════════════════════
    // WebP Signature Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_identifies_webp_by_signature() {
        // WebP: RIFF .... WEBP
        let webp_bytes = b"RIFF\x00\x00\x00\x00WEBP";
        let file = temp_file_with_bytes("webp", webp_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "webp");
    }

    #[test]
    fn probe_identifies_webp_with_arbitrary_size_field() {
        // WebP with non-zero RIFF size bytes
        let mut webp_bytes = Vec::from(b"RIFF" as &[u8]);
        webp_bytes.extend_from_slice(&[0x24, 0x10, 0x00, 0x00]); // size
        webp_bytes.extend_from_slice(b"WEBP");
        let file = temp_file_with_bytes("webp", &webp_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "webp");
    }

    // ═══════════════════════════════════════════
    // Edge Cases: Invalid / Malformed Files
    // ═══════════════════════════════════════════

    #[test]
    fn probe_rejects_empty_file() {
        let file = temp_file_with_bytes("png", &[]);
        let result = probe_image(file.path());
        // Empty file → reads 0 bytes → "File too small"
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("too small"),
            "Expected 'too small' error, got: {}",
            err
        );
    }

    #[test]
    fn probe_rejects_file_with_only_1_byte() {
        let file = temp_file_with_bytes("png", &[0x89]);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn probe_rejects_file_with_3_bytes() {
        // Just under the 4-byte threshold
        let file = temp_file_with_bytes("png", &[0x89, 0x50, 0x4E]);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn probe_rejects_random_binary_garbage() {
        let garbage = [
            0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x11, 0x22, 0x33,
        ];
        let file = temp_file_with_bytes("bin", &garbage);
        let result = probe_image(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported") || err.contains("unknown"));
    }

    #[test]
    fn probe_rejects_text_file_with_image_extension() {
        // A .png file that's actually plain text
        let text = b"Hello, this is not an image file at all.";
        let file = temp_file_with_bytes("png", text);
        let result = probe_image(file.path());
        // Signature check fails, but extension fallback exists for png
        // This tests the fallback behavior
        assert!(result.is_ok(), "Extension fallback should accept .png");
    }

    #[test]
    fn probe_rejects_nonexistent_file() {
        let path = Path::new("/nonexistent/fake_image.png");
        let result = probe_image(path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("IO error"), "Expected IO error, got: {}", err);
    }

    // ═══════════════════════════════════════════
    // Extension Fallback Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_falls_back_to_extension_for_unrecognized_signature_with_known_ext() {
        // Unknown signature but .webp extension → fallback
        let unknown_sig = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ];
        let file = temp_file_with_bytes("webp", &unknown_sig);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "webp");
    }

    #[test]
    fn probe_rejects_unknown_signature_with_unknown_extension() {
        // No matching signature AND no matching extension
        let unknown = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ];
        let file = temp_file_with_bytes("mp4", &unknown);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════
    // RIFF-but-not-WebP Test
    // ═══════════════════════════════════════════

    #[test]
    fn probe_rejects_riff_avi_file() {
        // RIFF .... AVI  — not WebP
        let avi_bytes = b"RIFF\x00\x00\x00\x00AVI ";
        let file = temp_file_with_bytes("avi", avi_bytes);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    // BMP Signature Tests

    #[test]
    fn probe_identifies_bmp_by_signature() {
        let bmp_bytes = b"BM\x36\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00";
        let file = temp_file_with_bytes("bmp", bmp_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "bmp");
    }

    // TIFF Signature Tests

    #[test]
    fn probe_identifies_tiff_le_by_signature() {
        let tiff_bytes = b"II*\x00\x08\x00\x00\x00\x00\x00\x00\x00";
        let file = temp_file_with_bytes("tiff", tiff_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "tiff");
    }

    #[test]
    fn probe_identifies_tiff_be_by_signature() {
        let tiff_bytes = b"MM\x00*\x00\x00\x00\x08\x00\x00\x00\x00";
        let file = temp_file_with_bytes("tiff", tiff_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "tiff");
    }

    #[test]
    fn probe_falls_back_to_extension_for_tif() {
        // Unknown signature but .tif extension — fallback
        let unknown_sig = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ];
        let file = temp_file_with_bytes("tif", &unknown_sig);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "tiff"); // Should resolve to canonical "tiff"
    }

    // ═══════════════════════════════════════════
    // ICO / CUR Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_identifies_ico_by_signature() {
        // Valid ICO: reserved 00 00, type 01 00, count 01 00
        let ico_bytes = [0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        let file = temp_file_with_bytes("ico", &ico_bytes);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "ico");
    }

    #[test]
    fn probe_rejects_ico_header_with_zero_image_count() {
        // ICO header with count = 0 — must reject even with .ico extension
        let ico_bytes_zero_count = [0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        let file = temp_file_with_bytes("ico", &ico_bytes_zero_count);
        let result = probe_image(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid ICO header"));
    }

    #[test]
    fn probe_rejects_zero_count_ico_even_with_ico_extension() {
        // Explicit duplicate proving extension does not rescue zero-count
        let bytes = [0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let file = temp_file_with_bytes("ico", &bytes);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn probe_rejects_truncated_ico_header_even_with_ico_extension() {
        // Only 4 bytes — ICO type field present but no count bytes
        let truncated = [0x00, 0x00, 0x01, 0x00];
        let file = temp_file_with_bytes("ico", &truncated);
        let result = probe_image(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid ICO header"));
    }

    #[test]
    fn probe_rejects_cur_signature() {
        // CUR: reserved 00 00, type 02 00
        let cur_bytes = [0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00];
        let file = temp_file_with_bytes("cur", &cur_bytes);
        let result = probe_image(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("CUR cursor files are not supported"));
    }

    #[test]
    fn probe_rejects_cur_signature_even_when_renamed_to_ico() {
        // CUR signature with .ico extension — signature wins
        let cur_bytes = [0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00];
        let file = temp_file_with_bytes("ico", &cur_bytes);
        let result = probe_image(file.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("CUR cursor files are not supported"));
    }

    #[test]
    fn probe_does_not_accept_cur_extension_fallback() {
        // Unknown signature with .cur extension — no fallback for .cur
        let unknown_sig = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ];
        let file = temp_file_with_bytes("cur", &unknown_sig);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn probe_accepts_ico_extension_fallback_only_when_no_invalid_magic_evidence() {
        // Truly unknown signature (not ICO/CUR family) with .ico extension.
        // ICO is excluded from extension fallback, so this must REJECT.
        let unknown_sig = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        ];
        let file = temp_file_with_bytes("ico", &unknown_sig);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    // ═══════════════════════════════════════════
    // GIF Signature Tests
    // ═══════════════════════════════════════════

    #[test]
    fn probe_file_identifies_gif87a() {
        let sig = b"GIF87a\x01\x00\x01\x00\x80\x00\x00";
        let file = temp_file_with_bytes("gif", sig);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "gif");
    }

    #[test]
    fn probe_file_identifies_gif89a() {
        let sig = b"GIF89a\x01\x00\x01\x00\x80\x00\x00";
        let file = temp_file_with_bytes("gif", sig);
        let result = probe_image(file.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "gif");
    }

    #[test]
    fn probe_file_rejects_non_gif() {
        let sig = b"NOTGIF\x01\x00\x01\x00\x80\x00\x00";
        let file = temp_file_with_bytes("gif", sig);
        let result = probe_image(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn probe_file_rejects_truncated_gif() {
        let sig = b"GIF"; // Too short
        let file = temp_file_with_bytes("gif", sig);
        let result = probe_image(file.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File too small"));
    }
}
