//! 模块与入口使用同一台 VM。宿主只提供冻结的包内容，没有运行时文件系统查找。
use super::source::{Modules, Script};
use mlua::{Error, Function, Lua, Result, Table, Value, chunk::ChunkMode};
use std::sync::Arc;

const CACHE: &str = "tank-arena.modules";

pub(super) fn install(lua: &Lua, script: &Script) -> Result<()> {
    lua.set_named_registry_value(CACHE, lua.create_table()?)?;
    lua.globals().raw_set(
        "require",
        require_from(lua, script.modules.clone(), "bot.luau".into())?,
    )
}

fn resolve(caller: &str, request: &str) -> Result<String> {
    if !(request.starts_with("./") || request.starts_with("../")) {
        return Err(Error::runtime(format!(
            "{caller}: require expects a relative path, got '{request}'"
        )));
    }
    let mut parts: Vec<_> = caller.split('/').collect();
    parts.pop();
    for part in request.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::runtime(format!(
                        "{caller}: '{request}' leaves the bot package"
                    )));
                }
            }
            name => parts.push(name),
        }
    }
    let mut path = parts.join("/");
    if !path.to_ascii_lowercase().ends_with(".luau") {
        path.push_str(".luau");
    }
    Ok(path)
}

fn require_from(lua: &Lua, files: Arc<Modules>, caller: String) -> Result<Function> {
    lua.create_function(move |lua, request: String| {
        let path = resolve(&caller, &request)?;
        if path == "bot.luau" {
            return Err(Error::runtime(format!(
                "{caller}: cannot require the bot entrypoint"
            )));
        }
        let src = files.get(&path).ok_or_else(|| {
            Error::runtime(format!(
                "{caller}: module '{path}' not found in the bot package"
            ))
        })?;
        let cache: Table = lua.named_registry_value(CACHE)?;
        let value: Value = cache.raw_get(path.as_str())?;
        // 用不向脚本暴露的缓存表自身作“正在加载”标记，不占用任何合法模块返回值。
        if value.as_table() == Some(&cache) {
            return Err(Error::runtime(format!(
                "{caller}: cyclic require of '{path}'"
            )));
        }
        if !value.is_nil() {
            return Ok(value);
        }

        // require 绑定到定义此 chunk 的文件，即使从别处调用它返回的函数，也不会丢失基目录。
        let env = lua.create_table()?;
        let mt = lua.create_table()?;
        mt.raw_set("__index", lua.globals())?;
        env.set_metatable(Some(mt))?;
        env.raw_set("require", require_from(lua, files.clone(), path.clone())?)?;
        cache.raw_set(path.as_str(), &cache)?;
        let loaded = lua
            .load(src.as_str())
            .set_name(format!("@{path}"))
            .set_mode(ChunkMode::Text)
            .set_environment(env)
            .call::<Value>(());
        match loaded {
            Ok(value) => {
                let value = if value.is_nil() {
                    Value::Boolean(true)
                } else {
                    value
                };
                cache.raw_set(path.as_str(), &value)?;
                Ok(value)
            }
            Err(e) => {
                cache.raw_set(path.as_str(), Value::Nil)?;
                Err(e)
            }
        }
    })
}
