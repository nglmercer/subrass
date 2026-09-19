use encoding_rs::{
    Encoding, BIG5, EUC_KR, GBK, SHIFT_JIS, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253,
    WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258, WINDOWS_874,
};

/// Map the numeric ASS/SSA `Encoding`/`\fe` value to the corresponding
/// Windows code page. Subtitle text arrives here as Rust Unicode, so the
/// compatibility bridge reinterprets contiguous byte-like Unicode values
/// (`U+0000..U+00FF`) as the original byte stream, including multibyte
/// encodings, while leaving already-Unicode scripts intact.
pub(super) fn ass_encoding(value: i32) -> Option<&'static Encoding> {
    match value {
        0 | 1 | 77 => Some(WINDOWS_1252),
        128 => Some(SHIFT_JIS),
        129 => Some(EUC_KR),
        130 => None, // Johab is not provided by encoding_rs.
        134 => Some(GBK),
        136 => Some(BIG5),
        161 => Some(WINDOWS_1253),
        162 => Some(WINDOWS_1254),
        163 => Some(WINDOWS_1258),
        177 => Some(WINDOWS_1255),
        178 => Some(WINDOWS_1256),
        186 => Some(WINDOWS_1257),
        204 => Some(WINDOWS_1251),
        222 => Some(WINDOWS_874),
        238 => Some(WINDOWS_1250),
        _ => None,
    }
}
