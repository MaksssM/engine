#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QualityPreset {
    Performance,
    #[default]
    Balanced,
    Quality,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderSettings {
    pub render_scale_percent: u32,
    pub texture_lod_bias: i32,
    pub shadow_quality: ShadowQuality,
    pub post_processing: bool,
    pub shader_variant: ShaderVariant,
    pub msaa_samples: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowQuality {
    Disabled,
    Low,
    High,
}

#[derive(Debug, Clone, Copy)]
pub enum ShaderVariant {
    Fast,
    Standard,
    Quality,
}

impl QualityPreset {
    pub fn from_environment() -> Self {
        match std::env::var("ENGINE_QUALITY").as_deref() {
            Ok("performance") => Self::Performance,
            Ok("quality") => Self::Quality,
            _ => Self::Balanced,
        }
    }

    pub const fn settings(self) -> RenderSettings {
        match self {
            Self::Performance => RenderSettings {
                render_scale_percent: 75,
                texture_lod_bias: 2,
                shadow_quality: ShadowQuality::Disabled,
                post_processing: false,
                shader_variant: ShaderVariant::Fast,
                msaa_samples: 1,
            },
            Self::Balanced => RenderSettings {
                render_scale_percent: 100,
                texture_lod_bias: 0,
                shadow_quality: ShadowQuality::Low,
                post_processing: true,
                shader_variant: ShaderVariant::Standard,
                msaa_samples: 1,
            },
            Self::Quality => RenderSettings {
                render_scale_percent: 100,
                texture_lod_bias: -1,
                shadow_quality: ShadowQuality::High,
                post_processing: true,
                shader_variant: ShaderVariant::Quality,
                msaa_samples: 4,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_expected_tradeoffs() {
        assert!(QualityPreset::Performance.settings().render_scale_percent < 100);
        assert!(matches!(
            QualityPreset::Performance.settings().shadow_quality,
            ShadowQuality::Disabled
        ));
        assert!(matches!(
            QualityPreset::Quality.settings().shadow_quality,
            ShadowQuality::High
        ));
    }
}
