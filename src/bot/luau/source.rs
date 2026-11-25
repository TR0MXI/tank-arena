use super::super::catalog;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::Path,
    sync::Arc,
};
use thiserror::Error;

const SOURCE_LIMIT: usize = 256 * 1024;

pub(super) type Modules = BTreeMap<String, String>;

#[derive(Clone, Debug)]
pub struct Script {
    name: Arc<str>,
    source: Arc<str>,
    digest: blake3::Hash,
    pub(super) modules: Arc<Modules>,
    author: Option<Arc<str>>,
    description: Option<Arc<str>>,
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("bot name cannot be empty")]
    EmptyName,
    #[error("expected a regular .luau file")]
    Extension,
    #[error("script exceeds 256 KiB")]
    Size,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl Script {
    /// 资料取自文件开头的单行注释，不为了读名字而编译或运行脚本。
    pub fn from_source(fallback: &str, source: &str) -> Result<Self, SourceError> {
        if source.len() > SOURCE_LIMIT {
            return Err(SourceError::Size);
        }

        let (mut name, mut author, mut description) = (None, None, None);
        for line in source.trim_start_matches('\u{feff}').lines().map(str::trim) {
            if line.is_empty() {
                continue;
            }
            let Some(comment) = line.strip_prefix("--") else {
                break;
            };

            // 资料头结束于代码或长注释（例如作者涂装），不匹配字符串里的标记。
            if comment
                .strip_prefix('[')
                .is_some_and(|s| s.trim_start_matches('=').starts_with('['))
            {
                break;
            }
            let Some((tag, value)) = comment.trim().split_once(char::is_whitespace) else {
                continue;
            };

            let (field, limit) = match tag {
                "@name" => (&mut name, 48),
                "@author" => (&mut author, 48),
                "@description" => (&mut description, 180),
                _ => continue,
            };
            let value = value.trim();
            if field.is_none() && !value.is_empty() && !value.chars().any(char::is_control) {
                *field = Some(Arc::<str>::from(
                    value.chars().take(limit).collect::<String>(),
                ));
            }
        }

        let name = name.unwrap_or_else(|| fallback.trim().into());
        if name.is_empty() {
            return Err(SourceError::EmptyName);
        }

        Ok(Self {
            name,
            author,
            description,
            source: source.into(),
            modules: Arc::default(),
            digest: blake3::hash(source.as_bytes()),
        })
    }

    /// 读取整份参赛快照。普通入口保持独立；目录策略的模块在此读入，而非在 VM 中读盘。
    pub fn read(path: &Path) -> Result<Self, SourceError> {
        let mut script = Self::read_entry(path)?;
        if path.file_name().is_some_and(|n| n == "bot.luau") {
            let absolute = std::path::absolute(path)?;
            let root = fs::canonicalize(absolute.parent().unwrap())?;
            // bots/bot.luau 本身也是合法的平铺车型，不应把整个车库当作其模块。
            if crate::local_files::bots_dir()?.canonicalize().ok().as_ref() != Some(&root) {
                let mut modules = Modules::new();
                collect_modules(&root, &root, &mut modules)?;
                if !modules.is_empty() {
                    script.digest = package_digest(script.text(), &modules);
                    script.modules = Arc::new(modules);
                }
            }
        }
        Ok(script)
    }

    /// 仅供车库显示入口资料/涂装；不需要读入任何模块。
    pub fn read_entry(path: &Path) -> Result<Self, SourceError> {
        if !catalog::is_luau(path) || !path.symlink_metadata()?.is_file() {
            return Err(SourceError::Extension);
        }
        Self::from_source(&catalog::tank_name(path), &read_text(path)?)
    }

    /// 回放身份不依赖安装目录或枚举顺序。单文件按源文本取摘要，包摘要还包含模块相对路径。
    pub fn blake3(&self) -> blake3::Hash {
        self.digest
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn text(&self) -> &str {
        &self.source
    }
}

fn read_text(path: &Path) -> Result<String, SourceError> {
    let mut text = String::new();
    File::open(path)?
        .take(SOURCE_LIMIT as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > SOURCE_LIMIT {
        return Err(SourceError::Size);
    }
    Ok(text)
}

fn collect_modules(root: &Path, dir: &Path, modules: &mut Modules) -> Result<(), SourceError> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let kind = item.file_type()?; // 不跟随符号链接。
        if item.file_name().as_encoded_bytes().starts_with(b".") {
            continue;
        }
        if kind.is_dir() {
            collect_modules(root, &path, modules)?;
        } else if kind.is_file() && catalog::is_luau(&path) {
            let rel = path.strip_prefix(root).unwrap();
            if rel == Path::new("bot.luau") {
                continue;
            }
            let name = rel
                .to_str()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "module path must be UTF-8")
                })?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let text = read_text(&path).map_err(|e| io::Error::other(format!("{name}: {e}")))?;
            modules.insert(name, text);
        }
    }
    Ok(())
}

fn package_digest(entry: &str, modules: &Modules) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    hash.update(b"tank-arena:luau-package:v1\0");
    for (path, text) in std::iter::once(("bot.luau", entry))
        .chain(modules.iter().map(|(p, t)| (p.as_str(), t.as_str())))
    {
        for field in [path, text] {
            hash.update(&(field.len() as u64).to_le_bytes());
            hash.update(field.as_bytes());
        }
    }
    hash.finalize()
}
