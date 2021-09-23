use spice_client_glib as spice;

pub(crate) fn mime_from_format(format: spice::ClipboardFormat) -> Option<&'static str> {
    match format {
        spice::ClipboardFormat::Utf8 => Some("text/plain;charset=utf-8"),
        spice::ClipboardFormat::Png => Some("image/png"),
        spice::ClipboardFormat::Bmp => Some("image/bmp"),
        spice::ClipboardFormat::Tiff => Some("image/tiff"),
        spice::ClipboardFormat::Jpg => Some("image/jpeg"),
        spice::ClipboardFormat::FileList => Some("text/uri-list"),
        _ => None,
    }
}

pub(crate) fn format_from_mime(mime: &str) -> Option<spice::ClipboardFormat> {
    match mime {
        "text/plain" => Some(spice::ClipboardFormat::Utf8),
        "text/plain;charset=utf-8" => Some(spice::ClipboardFormat::Utf8),
        _ => {
            log::debug!("Unhandled mime type: {}", mime);
            None
        }
    }
}
