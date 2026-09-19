pub mod attachment;
pub mod color;
pub mod color_space;
pub mod effect;
pub mod event;
pub mod override_tag;
pub mod script_info;
pub mod style;
pub mod time;

pub use attachment::{Attachment, AttachmentKind};
pub use color::Color;
pub use color_space::{
    convert_ass_rgb, ColorConversionError, VideoColorSpace, VideoMatrix, VideoRange,
};
pub use effect::LegacyEffect;
pub use event::{Event, EventType};
pub use override_tag::{parse_text_segments, OverrideTag, TextSegment};
pub use script_info::{ScriptInfo, ScriptType, YCbCrMatrix};
pub use style::Style;
pub use time::Time;
