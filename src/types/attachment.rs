use serde::{Deserialize, Serialize};

/// Which section an embedded attachment came from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    /// `[Fonts]` — an embedded TTF/OTF font
    Font,
    /// `[Graphics]` — an embedded image
    Graphic,
}

/// A binary attachment embedded in the ASS file ([Fonts] / [Graphics])
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub filename: String,
    /// Decoded binary payload (not serialized to JS; use the dedicated
    /// API accessor instead)
    #[serde(skip)]
    #[serde(default)]
    pub data: Vec<u8>,
}
