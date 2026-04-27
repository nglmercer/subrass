use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Script type version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum ScriptType {
    V400,         // SSA v4.00
    #[default]
    V400Plus,     // ASS v4.00+
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

/// YCbCr matrix type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum YCbCrMatrix {
    #[default]
    None,
    TV601,
    TV709,
    PC601,
    PC709,
}


impl std::fmt::Display for YCbCrMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::TV601 => write!(f, "TV.601"),
            Self::TV709 => write!(f, "TV.709"),
            Self::PC601 => write!(f, "PC.601"),
            Self::PC709 => write!(f, "PC.709"),
        }
    }
}

impl std::str::FromStr for YCbCrMatrix {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "None" | "none" => Ok(Self::None),
            "TV.601" | "tv.601" => Ok(Self::TV601),
            "TV.709" | "tv.709" => Ok(Self::TV709),
            "PC.601" | "pc.601" => Ok(Self::PC601),
            "PC.709" | "pc.709" => Ok(Self::PC709),
            _ => Err(format!("Unknown YCbCr matrix: {}", s)),
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
            y_cb_cr_matrix: YCbCrMatrix::None,
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
        }
    }
}

impl ScriptInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_field(&mut self, key: &str, value: &str) {
        match key.to_lowercase().as_str() {
            "title" => self.title = Some(value.to_string()),
            "scripttype" => {
                if let Ok(t) = value.parse() {
                    self.script_type = t;
                }
            }
            "playresx" => {
                if let Ok(v) = value.parse() {
                    self.play_res_x = v;
                }
            }
            "playresy" => {
                if let Ok(v) = value.parse() {
                    self.play_res_y = v;
                }
            }
            "layoutresx" => {
                if let Ok(v) = value.parse() {
                    self.layout_res_x = Some(v);
                }
            }
            "layoutresy" => {
                if let Ok(v) = value.parse() {
                    self.layout_res_y = Some(v);
                }
            }
            "scaledborderandshadow" => {
                self.scaled_border_and_shadow = value.trim().to_lowercase() == "yes";
            }
            "y cbcr matrix" | "ycbcr matrix" => {
                if let Ok(m) = value.parse() {
                    self.y_cb_cr_matrix = m;
                }
            }
            "wrapstyle" => {
                if let Ok(v) = value.parse() {
                    self.wrap_style = v;
                }
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
    }

    #[test]
    fn test_script_info_set_field() {
        let mut info = ScriptInfo::new();
        info.set_field("Title", "Test Subtitle");
        info.set_field("PlayResX", "1280");
        info.set_field("PlayResY", "720");

        assert_eq!(info.title.as_deref(), Some("Test Subtitle"));
        assert_eq!(info.play_res_x, 1280);
        assert_eq!(info.play_res_y, 720);
    }
}
