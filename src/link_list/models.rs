/// One result tile shown on the `/link-list` search page.
#[derive(Clone)]
pub struct ResultTile {
    /// `"plgb"` or `"tgfs"` — which database the file came from, used to build the detail link.
    pub source: &'static str,
    /// Opaque id used in `/link-list/file/{source}/{id}` (ObjectId hex for PLGB,
    /// `"{cluster}:{objectid_hex}"` entry id for TGFS).
    pub detail_id: String,
    pub file_name: String,
    pub file_size_fmt: String,
    pub icon: &'static str,
}

/// Full detail shown on the `/link-list/file/{source}/{id}` page.
#[derive(Clone)]
pub struct FileDetail {
    pub file_name: String,
    pub file_size_fmt: String,
    pub mime_type: String,
    pub icon: &'static str,
    pub dl_url: String,
    pub watch_url: String,
}

/// Picks a representative emoji for a file based on its mime type, falling back to the
/// file extension when the mime type is missing/generic (e.g. `application/octet-stream`,
/// which is what most Telegram uploads end up tagged as).
pub fn icon_for(mime_type: &str, file_name: &str) -> &'static str {
    let mime = mime_type.to_lowercase();
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();

    if mime.starts_with("video/")
        || matches!(ext.as_str(), "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "3gp")
    {
        return "🎬";
    }
    if mime.starts_with("audio/") || matches!(ext.as_str(), "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma") {
        return "🎵";
    }
    if mime.starts_with("image/")
        || matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "heic")
    {
        return "🖼️";
    }
    if mime == "application/pdf" || ext == "pdf" {
        return "📕";
    }
    if matches!(ext.as_str(), "exe" | "msi" | "apk" | "app" | "dmg" | "deb" | "appimage")
        || mime.contains("android.package-archive")
        || mime.contains("x-msdownload")
        || mime.contains("x-executable")
    {
        return "⚙️";
    }
    if matches!(ext.as_str(), "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz")
        || mime.contains("zip")
        || mime.contains("compressed")
        || mime.contains("archive")
    {
        return "🗜️";
    }
    if matches!(ext.as_str(), "doc" | "docx") || mime.contains("wordprocessingml") || mime.contains("msword") {
        return "📝";
    }
    if matches!(ext.as_str(), "xls" | "xlsx") || mime.contains("spreadsheetml") || mime.contains("ms-excel") {
        return "📊";
    }
    if matches!(ext.as_str(), "ppt" | "pptx") || mime.contains("presentationml") || mime.contains("ms-powerpoint") {
        return "📽️";
    }
    if mime.starts_with("text/") || matches!(ext.as_str(), "txt" | "log" | "csv" | "json" | "xml") {
        return "📄";
    }
    "📁"
}
