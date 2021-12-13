use std::{
    str::{from_utf8, Utf8Error},
    string::FromUtf16Error,
};

pub(crate) fn mime_from_format(format: freerdp::channels::cliprdr::Format) -> Option<&'static str> {
    use freerdp::channels::cliprdr::Format;

    match format {
        Format::Text | Format::OemText | Format::UnicodeText => Some("text/plain;charset=utf-8"),
        Format::Dib | Format::DibV5 => Some("image/bmp"),
        Format::Html => Some("text/html"),
        Format::Png => Some("image/png"),
        Format::Jpeg => Some("image/jpeg"),
        Format::Gif => Some("image/gif"),
        Format::TextUriList => Some("text/uri-list"),
        _ => None,
    }
}

pub(crate) fn format_from_mime(format: &str) -> Option<freerdp::channels::cliprdr::Format> {
    use freerdp::channels::cliprdr::Format;

    match format {
        "text/plain" | "text/plain;charset=utf-8" | "UTF8_STRING" | "TEXT" | "STRING" => {
            Some(Format::UnicodeText)
        }
        "image/bmp" => Some(Format::Dib),
        "text/html" => Some(Format::Html),
        "image/png" => Some(Format::Png),
        "image/jpeg" => Some(Format::Jpeg),
        "image/gif" => Some(Format::Gif),
        "text/uri-list" => Some(Format::TextUriList),
        _ => None,
    }
}

pub(crate) fn string_from_utf16(data: Vec<u8>) -> Result<String, FromUtf16Error> {
    let utf16: Vec<u16> = data
        .chunks_exact(2)
        .into_iter()
        .map(|a| u16::from_ne_bytes([a[0], a[1]]))
        .collect();
    String::from_utf16(&utf16)
}

pub(crate) fn utf16_from_utf8(data: &[u8]) -> Result<Vec<u8>, Utf8Error> {
    let utf8 = from_utf8(data)?;
    let utf16 = utf8.encode_utf16().flat_map(u16::to_ne_bytes).collect();
    Ok(utf16)
}
