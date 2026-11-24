//! 图片字节处理：base64 解码、格式识别、不覆盖地落盘、`~` 展开。

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
}

impl ImageFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Gif => "gif",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::Webp => "WEBP",
            ImageFormat::Gif => "GIF",
        }
    }
}

/// 按文件头识别图片格式；非图片返回 `None`。
pub fn detect_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else {
        None
    }
}

const LENIENT: GeneralPurposeConfig =
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
const STANDARD_LENIENT: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, LENIENT);
const URL_SAFE_LENIENT: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::URL_SAFE, LENIENT);

/// 解码 base64；兼容 `data:image/...;base64,` 前缀、换行空白、URL-safe 字母表与缺失填充。
pub fn decode_base64(s: &str) -> Result<Vec<u8>, String> {
    let payload = match s.trim_start().strip_prefix("data:") {
        Some(rest) => rest.split_once(',').map(|(_, b)| b).ok_or("data URI 缺少逗号")?,
        None => s,
    };
    let compact: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD_LENIENT
        .decode(&compact)
        .or_else(|_| URL_SAFE_LENIENT.decode(&compact))
        .map_err(|e| format!("base64 解码失败：{e}"))
}

/// 把开头的 `~` 展开为用户主目录。
pub fn expand_tilde(p: &str) -> PathBuf {
    let home = dirs::home_dir();
    match (p, home) {
        ("~", Some(h)) => h,
        (_, Some(h)) if p.starts_with("~/") => h.join(&p[2..]),
        _ => PathBuf::from(p),
    }
}

/// 保存为 `{dir}/{stamp}.{ext}`，`stamp` 为 `yyyyMMddHHmmssSSS`；重名追加 `_1`、`_2`…，绝不覆盖；目录不存在时创建。
pub fn save_image(dir: &Path, bytes: &[u8], format: ImageFormat, stamp: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建图片目录 {}：{e}", dir.display()))?;
    let ext = format.ext();
    for n in 0usize.. {
        let name = if n == 0 { format!("{stamp}.{ext}") } else { format!("{stamp}_{n}.{ext}") };
        let path = dir.join(name);
        // create_new 原子地保证不覆盖已有文件（含并发写入）
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(bytes).map_err(|e| format!("写入图片失败 {}：{e}", path.display()))?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("写入图片失败 {}：{e}", path.display())),
        }
    }
    unreachable!("unbounded range")
}
#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";
    const JPG: &[u8] = b"\xff\xd8\xff\xe0rest";
    const WEBP: &[u8] = b"RIFF\0\0\0\0WEBPVP8 ";
    const GIF: &[u8] = b"GIF89a....";

    #[test]
    fn detects_formats() {
        assert_eq!(detect_format(PNG), Some(ImageFormat::Png));
        assert_eq!(detect_format(JPG), Some(ImageFormat::Jpeg));
        assert_eq!(detect_format(WEBP), Some(ImageFormat::Webp));
        assert_eq!(detect_format(GIF), Some(ImageFormat::Gif));
        assert_eq!(detect_format(b"<!DOCTYPE html>"), None); // BN-011
        assert_eq!(detect_format(b""), None);
        assert_eq!(ImageFormat::Jpeg.ext(), "jpg");
        assert_eq!(ImageFormat::Webp.name(), "WEBP");
    }

    // AC-007
    #[test]
    fn decodes_variants() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("data:image/png;base64,aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVs\nbG8=\n").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(decode_base64("-_8").unwrap(), vec![0xfb, 0xff]);
        assert!(decode_base64("%%%not base64").is_err());
    }

    #[test]
    fn expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/.images/"), home.join(".images/"));
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("/abs/p"), PathBuf::from("/abs/p"));
    }

    // AC-004, BN-002
    #[test]
    fn saves_without_overwrite() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("new").join("dir");
        let a = save_image(&dir, PNG, ImageFormat::Png, "20261116151516723").unwrap();
        assert_eq!(a, dir.join("20261116151516723.png"));
        let b = save_image(&dir, JPG, ImageFormat::Jpeg, "20261116151516723").unwrap();
        assert_eq!(b, dir.join("20261116151516723.jpg"));
        let c = save_image(&dir, PNG, ImageFormat::Png, "20261116151516723").unwrap();
        assert_eq!(c, dir.join("20261116151516723_1.png"));
        let d = save_image(&dir, PNG, ImageFormat::Png, "20261116151516723").unwrap();
        assert_eq!(d, dir.join("20261116151516723_2.png"));
        assert_eq!(std::fs::read(&a).unwrap(), PNG);
        assert_eq!(std::fs::read(&c).unwrap(), PNG);
    }
}
