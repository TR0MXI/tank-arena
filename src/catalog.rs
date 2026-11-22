//! 发现本地机器人入口，不读取或执行脚本内容。

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

/// 按稳定顺序返回入口路径，以路径而非显示名区分模型。
/// 例如 `owl.luau` 和 `owl/bot.luau` 默认都显示为「owl」，但仍是两个选项。
pub fn discover(bots_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let items = match fs::read_dir(bots_dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(entries),
        result => result?,
    };
    for item in items {
        let item = item?;
        let kind = match item.file_type() {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            result => result?,
        };
        let path = item.path();
        if kind.is_file() && is_luau(&path) {
            entries.push(path);
        } else if kind.is_dir() {
            let entry = path.join("bot.luau");
            match fs::symlink_metadata(&entry) {
                Ok(meta) if meta.is_file() => entries.push(entry),
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
    }

    entries.sort();
    Ok(entries)
}

pub(crate) fn is_luau(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(OsStr::new("luau")))
}

/// 脚本未声明名字时使用。目录根部的 `bot.luau` 仍是单文件，而非包入口。
pub fn tank_name(path: &Path) -> String {
    let root = crate::local_files::bots_dir().ok();
    let path = root
        .as_deref()
        .and_then(|dir| path.strip_prefix(dir).ok())
        .or_else(|| path.strip_prefix("bots").ok())
        .unwrap_or(path);
    let name = if path.file_name().is_some_and(|f| f == "bot.luau") {
        path.parent().and_then(Path::file_name).or(path.file_stem())
    } else {
        path.file_stem()
    };
    name.unwrap_or_default().to_string_lossy().into_owned()
}
