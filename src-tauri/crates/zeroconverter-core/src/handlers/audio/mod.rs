pub mod execute;
pub mod formats;
pub mod pcm;
pub mod probe;
pub mod shape;
pub mod validate;

use crate::app_error::AppError;
use crate::conversion::traits::ConverterHandler;
use crate::conversion::types::{
    ConversionCapability, ConversionRequest, ConversionResult, FormatDescriptor, InputFormat,
    OutputFormat,
};
use std::path::Path;
use std::sync::OnceLock;

/// All supported audio descriptors, built once from the authoritative registry.
pub fn all_descriptors() -> &'static [FormatDescriptor] {
    static ALL: OnceLock<Vec<FormatDescriptor>> = OnceLock::new();
    ALL.get_or_init(|| formats::MODULES.iter().map(|m| m.descriptor).collect())
}

pub struct AudioHandler;

impl ConverterHandler for AudioHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        all_descriptors()
    }

    fn probe(&self, path: &Path) -> Result<InputFormat, AppError> {
        probe::probe_audio(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let format = formats::find(&ext)
            .ok_or_else(|| AppError::Probe(format!("Unsupported audio extension: {ext}")))?;
        if !format.descriptor.can_read {
            return Err(AppError::Probe(format!(
                "Audio format is not readable: {ext}"
            )));
        }
        Ok(InputFormat(format.descriptor.id.to_owned()))
    }

    fn capabilities(&self) -> Vec<ConversionCapability> {
        formats::MODULES
            .iter()
            .filter(|module| module.descriptor.can_read)
            .map(|source| ConversionCapability {
                input: InputFormat(source.descriptor.id.to_owned()),
                outputs: formats::MODULES
                    .iter()
                    .filter(|dest| {
                        dest.encoder.is_some() && source.descriptor.id != dest.descriptor.id
                    })
                    .map(|dest| OutputFormat(dest.descriptor.id.to_owned()))
                    .collect(),
            })
            .collect()
    }

    fn validate(&self, request: &ConversionRequest) -> Result<(), AppError> {
        validate::validate_audio(request)
    }

    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        execute::execute_audio(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_capabilities_match_encoder_registry() {
        let handler = AudioHandler;
        let caps = handler.capabilities();
        assert_eq!(handler.format_descriptors().len(), formats::MODULES.len());
        for module in formats::MODULES {
            let cap = caps
                .iter()
                .find(|cap| cap.input.0 == module.descriptor.id)
                .expect("every readable audio module is a capability input");
            for output in &cap.outputs {
                let encoder = formats::find(&output.0).expect("output module exists");
                assert!(encoder.encoder.is_some());
                assert_ne!(cap.input.0, output.0);
            }
        }
    }
}
