use crate::*;

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PromptImageType {
    #[serde(rename = "image")]
    Image,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PromptImageMimeType {
    #[serde(rename = "image/jpeg")]
    ImageJpeg,
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "image/webp")]
    ImageWebp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptImage {
    pub r#type: PromptImageType,
    pub mime_type: PromptImageMimeType,
    pub data: String,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum ViewImageMimeType {
    #[serde(rename = "image/jpeg")]
    ImageJpeg,
    #[serde(rename = "image/png")]
    ImagePng,
    #[serde(rename = "image/webp")]
    ImageWebp,
    #[serde(rename = "image/gif")]
    ImageGif,
}

/// On-demand display image; unlike prompt previews, GIF is supported too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ViewImage {
    pub mime_type: ViewImageMimeType,
    pub data: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub width: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub height: Option<u64>,
}
