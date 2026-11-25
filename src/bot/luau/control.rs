use crate::sim::{ControlUpdate, Drive, Turn};
use mlua::{Error, LuaString, Result, Value};

#[derive(Debug, thiserror::Error)]
#[error("invalid control update: {0}")]
pub(super) struct BadAction(&'static str);

fn bad(msg: &'static str) -> Error {
    Error::external(BadAction(msg))
}

pub(super) fn parse(value: Value) -> Result<ControlUpdate> {
    let mut update = ControlUpdate::default();
    let table = match value {
        Value::Nil => return Ok(update),
        Value::Table(t) => t,
        _ => return Err(bad("decide(obs) must return a table or nil")),
    };
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = key else {
            return Err(bad("expected named fields: drive, turn, fire"));
        };

        match key.as_bytes().as_ref() {
            b"drive" => {
                update.drive = Some(
                    match value.as_string().map(LuaString::as_bytes).as_deref() {
                        Some(b"forward") => Drive::Forward,
                        Some(b"reverse") => Drive::Reverse,
                        Some(b"stop") => Drive::Stop,
                        _ => return Err(bad("drive must be 'forward', 'reverse', or 'stop'")),
                    },
                );
            }
            b"turn" => {
                update.turn = Some(
                    match value.as_string().map(LuaString::as_bytes).as_deref() {
                        Some(b"left") => Turn::Left,
                        Some(b"right") => Turn::Right,
                        Some(b"none") => Turn::None,
                        _ => return Err(bad("turn must be 'left', 'right', or 'none'")),
                    },
                );
            }
            b"fire" => {
                update.fire = Some(
                    value
                        .as_boolean()
                        .ok_or_else(|| bad("fire must be true or false"))?,
                );
            }
            _ => return Err(bad("unknown field; expected drive, turn, or fire")),
        }
    }

    Ok(update)
}
