use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};
use thiserror::Error;

const SOURCE_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct Script {
    name: Arc<str>,
    source: Arc<str>,
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
        })
    }

    pub fn read(path: &Path) -> Result<Self, SourceError> {
        if !crate::catalog::is_luau(path) || !path.metadata()?.is_file() {
            return Err(SourceError::Extension);
        }

        let name = crate::catalog::tank_name(path);
        let mut s = String::new();
        File::open(path)?
            .take(SOURCE_LIMIT as u64 + 1)
            .read_to_string(&mut s)?;
        Self::from_source(&name, &s)
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
