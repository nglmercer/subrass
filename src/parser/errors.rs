/// Parser error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
    #[error("Invalid section header: {0}")]
    InvalidSection(String),

    #[error("Missing format line in section: {0}")]
    MissingFormatLine(String),

    #[error("Invalid format line: {0}")]
    InvalidFormatLine(String),

    #[error("Field count mismatch: expected {expected}, got {actual}")]
    FieldCountMismatch { expected: usize, actual: usize },

    #[error("Invalid time format: {0}")]
    InvalidTime(String),

    #[error("Invalid color format: {0}")]
    InvalidColor(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for field {field}: {value}")]
    InvalidValue { field: String, value: String },

    #[error("Unsupported script type: {0}")]
    UnsupportedScriptType(String),

    #[error("Parse error at line {line}: {message}")]
    LineError { line: usize, message: String },

    #[error("Unexpected error: {0}")]
    Unexpected(String),
}

impl ParseError {
    pub fn line_error(line: usize, message: impl Into<String>) -> Self {
        Self::LineError {
            line,
            message: message.into(),
        }
    }
}

/// Document section types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    ScriptInfo,
    V4Styles,
    V4PlusStyles,
    Events,
    Fonts,
    Graphics,
}

impl Section {
    pub fn from_header(header: &str) -> Option<Self> {
        let header = header.trim().to_lowercase();
        match header.as_str() {
            "[script info]" | "[scriptinfo]" => Some(Self::ScriptInfo),
            "[v4 styles]" => Some(Self::V4Styles),
            "[v4+ styles]" => Some(Self::V4PlusStyles),
            "[events]" => Some(Self::Events),
            "[fonts]" => Some(Self::Fonts),
            "[graphics]" => Some(Self::Graphics),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_from_header() {
        assert_eq!(
            Section::from_header("[Script Info]"),
            Some(Section::ScriptInfo)
        );
        assert_eq!(
            Section::from_header("[V4+ Styles]"),
            Some(Section::V4PlusStyles)
        );
        assert_eq!(Section::from_header("[Events]"), Some(Section::Events));
        assert_eq!(Section::from_header("[Fonts]"), Some(Section::Fonts));
        assert_eq!(Section::from_header("[invalid]"), None);
    }

    #[test]
    fn test_parse_error_display() {
        let error = ParseError::InvalidTime("bad".to_string());
        assert!(error.to_string().contains("Invalid time format"));

        let error = ParseError::line_error(42, "test error");
        assert!(error.to_string().contains("line 42"));
    }
}
