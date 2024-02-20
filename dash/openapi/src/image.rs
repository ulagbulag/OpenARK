#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub codec: ImageCodec,
    pub index: usize,
    #[serde(flatten)]
    pub size: ImageSize,
}
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

#[cfg_attr(feature = "clap", derive(::clap::ValueEnum))]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum ImageCodec {
    #[default]
    Jpeg,
}

impl ImageCodec {
    pub const fn as_fourcc(&self) -> &[u8; 4] {
        match self {
            Self::Jpeg => b"MJPG",
        }
    }

    pub const fn as_extension(&self) -> &'static str {
        match self {
            Self::Jpeg => ".jpeg",
        }
    }
}
