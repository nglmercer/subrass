use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Script type version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ScriptType {
    V400, // SSA v4.00
    #[default]
    V400Plus, // ASS v4.00+
    V400PlusPlus, // ASS2 v4.00++ (rare)
}

impl std::fmt::Display for ScriptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V400 => write!(f, "v4.00"),
            Self::V400Plus => write!(f, "v4.00+"),
            Self::V400PlusPlus => write!(f, "v4.00++"),
        }
    }
}

impl std::str::FromStr for ScriptType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "v4.00" => Ok(Self::V400),
            "v4.00+" => Ok(Self::V400Plus),
            "v4.00++" => Ok(Self::V400PlusPlus),
            _ => Err(format!("Unknown script type: {}", s)),
        }
    }
}

/// ASS/libass YCbCr matrix metadata.
///
/// This is metadata for the host/video integration boundary. The subtitle
/// renderer itself emits authored RGB values unchanged; `Default` maps to
/// libass's default TV.601 source matrix only when an explicit downstream
/// conversion is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum YCbCrMatrix {
    #[default]
    Default,
    Unknown,
    None,
    TV601,
    PC601,
    TV709,
    PC709,
    TV240M,
    PC240M,
    TVFCC,
    PCFCC,
}

impl std::fmt::Display for YCbCrMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "Default"),
            Self::Unknown => write!(f, "Unknown"),
            Self::None => write!(f, "None"),
            Self::TV601 => write!(f, "TV.601"),
            Self::PC601 => write!(f, "PC.601"),
            Self::TV709 => write!(f, "TV.709"),
            Self::PC709 => write!(f, "PC.709"),
            Self::TV240M => write!(f, "TV.240m"),
            Self::PC240M => write!(f, "PC.240m"),
            Self::TVFCC => write!(f, "TV.FCC"),
            Self::PCFCC => write!(f, "PC.FCC"),
        }
    }
}

impl std::str::FromStr for YCbCrMatrix {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "" => Ok(Self::Default),
            "unknown" => Ok(Self::Unknown),
            "none" => Ok(Self::None),
            "tv.601" => Ok(Self::TV601),
            "pc.601" => Ok(Self::PC601),
            "tv.709" => Ok(Self::TV709),
            "pc.709" => Ok(Self::PC709),
            "tv.240m" => Ok(Self::TV240M),
            "pc.240m" => Ok(Self::PC240M),
            "tv.fcc" => Ok(Self::TVFCC),
            "pc.fcc" => Ok(Self::PCFCC),
            // libass keeps parsing the document when the metadata is not
            // recognized. Preserve that state explicitly for callers that
            // need to decide how (or whether) to convert authored RGB.
            _ => Ok(Self::Unknown),
        }
    }
}

/// Script information from [Script Info] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub title: Option<String>,
    pub script_type: ScriptType,
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub layout_res_x: Option<u32>,
    pub layout_res_y: Option<u32>,
    pub scaled_border_and_shadow: bool,
    /// libass `Kerning:` header (default off: `ass_new_track` uses
    /// `calloc`, so `track->Kerning` is 0 unless the script enables it).
    pub kerning: bool,
    pub y_cb_cr_matrix: YCbCrMatrix,
    pub wrap_style: u32,
    pub original_script: Option<String>,
    pub original_translation: Option<String>,
    pub original_editing: Option<String>,
    pub original_timing: Option<String>,
    pub sync_point: Option<String>,
    pub updated_by: Option<String>,
    pub update_date: Option<String>,
    pub comment: Option<String>,
    pub extra_fields: HashMap<String, String>,
    /// Raw values retained by the byte-oriented parser until the document's
    /// style encoding is known. This is omitted from serialized/WASM output.
    #[serde(skip)]
    pub(crate) source_fields: HashMap<String, Vec<u8>>,
}

impl Default for ScriptInfo {
    fn default() -> Self {
        Self {
            title: None,
            script_type: ScriptType::V400Plus,
            play_res_x: 1920,
            play_res_y: 1080,
            layout_res_x: None,
            layout_res_y: None,
            scaled_border_and_shadow: true,
            kerning: false,
            y_cb_cr_matrix: YCbCrMatrix::Default,
            wrap_style: 0,
            original_script: None,
            original_translation: None,
            original_editing: None,
            original_timing: None,
            sync_point: None,
            updated_by: None,
            update_date: None,
            comment: None,
            extra_fields: HashMap::new(),
            source_fields: HashMap::new(),
        }
    }
}

impl ScriptInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retain a raw Script Info value while parsing bytes. Values are
    /// decoded later once a style encoding provides the legacy-code-page hint.
    pub(crate) fn retain_source_field(&mut self, key: &str, value: &[u8]) {
        self.source_fields
            .insert(key.to_ascii_lowercase(), value.to_vec());
    }

    /// Re-decode byte-preserved metadata after the style table is available.
    /// Numeric and enum fields are ASCII and therefore remain unchanged.
    pub(crate) fn redecode_source_fields(&mut self, encoding: i32) {
        let fields = self.source_fields.clone();
        for (key, value) in fields {
            let decoded = crate::charset::decode_metadata_bytes(&value, encoding);
            // The original parse already validated the field. Re-decoding can
            // only change its textual representation.
            let _ = self.set_field(&key, &decoded);
        }
    }

    /// Set a known field, validating the value. Unknown keys are stored
    /// in `extra_fields`. YCbCr metadata follows libass's tolerant parsing:
    /// an empty value selects `Default`, while an unrecognized value selects
    /// `Unknown` instead of rejecting the document.
    pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String> {
        let parse_res = |what: &str, v: &str| -> Result<u32, String> {
            let n: u32 = v
                .trim()
                .parse()
                .map_err(|_| format!("Invalid {} value: {}", what, v))?;
            if n == 0 {
                return Err(format!("Invalid {} value (must be positive): {}", what, v));
            }
            Ok(n)
        };
        match key.to_lowercase().as_str() {
            "title" => self.title = Some(value.to_string()),
            "scripttype" => {
                self.script_type = value.parse().map_err(|e: String| e)?;
            }
            "playresx" => {
                self.play_res_x = parse_res("PlayResX", value)?;
            }
            "playresy" => {
                self.play_res_y = parse_res("PlayResY", value)?;
            }
            "layoutresx" => {
                self.layout_res_x = Some(parse_res("LayoutResX", value)?);
            }
            "layoutresy" => {
                self.layout_res_y = Some(parse_res("LayoutResY", value)?);
            }
            "scaledborderandshadow" => {
                self.scaled_border_and_shadow = match value.trim().to_lowercase().as_str() {
                    "yes" | "true" | "1" => true,
                    "no" | "false" | "0" => false,
                    _ => {
                        return Err(format!(
                            "Invalid ScaledBorderAndShadow value (expected yes/no): {}",
                            value
                        ))
                    }
                };
            }
            "kerning" => {
                self.kerning = match value.trim().to_lowercase().as_str() {
                    "yes" | "true" | "1" => true,
                    "no" | "false" | "0" => false,
                    _ => {
                        return Err(format!(
                            "Invalid Kerning value (expected yes/no): {}",
                            value
                        ))
                    }
                };
            }
            "y cbcr matrix" | "ycbcr matrix" => {
                self.y_cb_cr_matrix = value.parse().map_err(|e: String| e)?;
            }
            "wrapstyle" => {
                let v: u32 = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("Invalid WrapStyle value: {}", value))?;
                if v > 3 {
                    return Err(format!("Invalid WrapStyle value (expected 0-3): {}", value));
                }
                self.wrap_style = v;
            }
            "original script" => self.original_script = Some(value.to_string()),
            "original translation" => self.original_translation = Some(value.to_string()),
            "original editing" => self.original_editing = Some(value.to_string()),
            "original timing" => self.original_timing = Some(value.to_string()),
            "sync point" => self.sync_point = Some(value.to_string()),
            "updated by" => self.updated_by = Some(value.to_string()),
            "update date" => self.update_date = Some(value.to_string()),
            "comment" => self.comment = Some(value.to_string()),
            _ => {
                self.extra_fields.insert(key.to_string(), value.to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_type_parsing() {
        assert_eq!("v4.00".parse::<ScriptType>().unwrap(), ScriptType::V400);
        assert_eq!(
            "v4.00+".parse::<ScriptType>().unwrap(),
            ScriptType::V400Plus
        );
        assert_eq!(
            "v4.00++".parse::<ScriptType>().unwrap(),
            ScriptType::V400PlusPlus
        );
    }

    #[test]
    fn test_script_info_defaults() {
        let info = ScriptInfo::default();
        assert_eq!(info.play_res_x, 1920);
        assert_eq!(info.play_res_y, 1080);
        assert!(info.scaled_border_and_shadow);
        assert!(!info.kerning);
        assert_eq!(info.y_cb_cr_matrix, YCbCrMatrix::Default);
    }

    #[test]
    fn test_kerning_header_parses_like_libass_bool() {
        let mut info = ScriptInfo::default();
        for (value, expected) in [
            ("yes", true),
            ("Yes", true),
            ("1", true),
            ("no", false),
            ("0", false),
        ] {
            info.set_field("Kerning", value).unwrap();
            assert_eq!(info.kerning, expected, "{value}");
        }
        assert!(info.set_field("Kerning", "maybe").is_err());
    }

    #[test]
    fn test_script_info_set_field() {
        let mut info = ScriptInfo::new();
        info.set_field("Title", "Test Subtitle").unwrap();
        info.set_field("PlayResX", "1280").unwrap();
        info.set_field("PlayResY", "720").unwrap();

        assert_eq!(info.title.as_deref(), Some("Test Subtitle"));
        assert_eq!(info.play_res_x, 1280);
        assert_eq!(info.play_res_y, 720);
    }

    #[test]
    fn test_set_field_rejects_invalid_known_values() {
        let mut info = ScriptInfo::new();
        assert!(info.set_field("PlayResX", "abc").is_err());
        assert!(info.set_field("PlayResX", "0").is_err());
        assert!(info.set_field("PlayResY", "-5").is_err());
        assert!(info.set_field("WrapStyle", "9").is_err());
        assert!(info.set_field("ScriptType", "v9").is_err());
        info.set_field("YCbCr Matrix", "bogus").unwrap();
        assert_eq!(info.y_cb_cr_matrix, YCbCrMatrix::Unknown);
        assert!(info.set_field("ScaledBorderAndShadow", "maybe").is_err());
        // Defaults preserved after rejected writes
        assert_eq!(info.play_res_x, 1920);
        // Unknown keys still accepted
        assert!(info.set_field("CustomKey", "anything").is_ok());
        assert_eq!(info.extra_fields.get("CustomKey").unwrap(), "anything");
    }

    #[test]
    fn test_all_libass_ycbcr_matrix_values_parse() {
        for (text, expected) in [
            ("Default", YCbCrMatrix::Default),
            ("Unknown", YCbCrMatrix::Unknown),
            ("None", YCbCrMatrix::None),
            ("TV.601", YCbCrMatrix::TV601),
            ("PC.601", YCbCrMatrix::PC601),
            ("TV.709", YCbCrMatrix::TV709),
            ("PC.709", YCbCrMatrix::PC709),
            ("TV.240m", YCbCrMatrix::TV240M),
            ("PC.240m", YCbCrMatrix::PC240M),
            ("TV.FCC", YCbCrMatrix::TVFCC),
            ("PC.FCC", YCbCrMatrix::PCFCC),
        ] {
            assert_eq!(text.parse::<YCbCrMatrix>().unwrap(), expected, "{text}");
            assert_eq!(expected.to_string(), text);
        }
        assert_eq!(
            "tv.240M".parse::<YCbCrMatrix>().unwrap(),
            YCbCrMatrix::TV240M
        );
        assert_eq!("".parse::<YCbCrMatrix>().unwrap(), YCbCrMatrix::Default);
        assert_eq!("  \t".parse::<YCbCrMatrix>().unwrap(), YCbCrMatrix::Default);
        assert_eq!(
            "bogus".parse::<YCbCrMatrix>().unwrap(),
            YCbCrMatrix::Unknown
        );
    }

    #[test]
    fn test_ycbcr_matrix_parser_is_case_and_whitespace_tolerant() {
        for (text, expected) in [
            ("  tV.601  ", YCbCrMatrix::TV601),
            ("pC.709", YCbCrMatrix::PC709),
            (" Tv.240M ", YCbCrMatrix::TV240M),
            ("PC.fCc", YCbCrMatrix::PCFCC),
        ] {
            assert_eq!(text.parse::<YCbCrMatrix>().unwrap(), expected, "{text:?}");
        }
    }
}
