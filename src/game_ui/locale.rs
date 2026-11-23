use super::*;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use tank_arena::sim::{Item, ItemPool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(super) enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

impl Language {
    fn code(self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::English => "en",
        }
    }

    fn activate(self) {
        rust_i18n::set_locale(self.code());
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Preferences {
    language: Language,
    #[serde(default = "all_items", skip_serializing_if = "is_all_items")]
    pub(super) items: ItemPool,
}
fn all_items() -> ItemPool {
    ItemPool::ALL
}
fn is_all_items(v: &ItemPool) -> bool {
    *v == ItemPool::ALL
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::default(),
            items: ItemPool::ALL,
        }
    }
}
impl Preferences {
    pub(super) fn toggle_crate(&mut self, k: Item) -> io::Result<()> {
        self.items.toggle(k);
        save(&path()?, self)
    }
}

fn path() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("local", "Tank Arena", "Tank Arena")
        .map(|p| p.config_dir().join("preferences.json"))
        .ok_or_else(|| io::Error::other("no preferences directory"))
}

fn read(p: &Path) -> Preferences {
    fs::read(p)
        .ok()
        .and_then(|b| serde_json::from_slice::<Preferences>(&b).ok())
        .unwrap_or_default()
}

fn save(p: &Path, settings: &Preferences) -> io::Result<()> {
    let dir = p
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut tmp, settings)?;
    tmp.persist(p)?;
    Ok(())
}

pub(super) fn tr(key: &str) -> String {
    rust_i18n::t!(key).into_owned()
}

pub(super) fn save_error(e: &tank_arena::replay::SaveFailure) -> String {
    use tank_arena::replay::SaveFailure;

    tr(match e {
        SaveFailure::QueueFull => "saveQueueFull",
        SaveFailure::Interrupted => "saveInterrupted",
        SaveFailure::Io(e) => match e.kind() {
            io::ErrorKind::PermissionDenied => "saveDenied",
            io::ErrorKind::StorageFull => "saveNoSpace",
            io::ErrorKind::AlreadyExists => "saveExists",
            io::ErrorKind::Interrupted => "saveInterrupted",
            _ => "saveWriteError",
        },
    })
}

pub(super) fn replay_error(e: &io::Error) -> String {
    use tank_arena::replay::{ReplayIssue, ReplayProblem};

    if let Some(issue) = e.get_ref().and_then(|e| e.downcast_ref::<ReplayIssue>()) {
        let reason = tr(match issue.problem {
            ReplayProblem::Format => "replayFormat",
            ReplayProblem::Version => "replayVersion",
            ReplayProblem::Configuration => "replayConfig",
            ReplayProblem::Truncated => "replayTruncated",
            ReplayProblem::Sequence => "replaySequence",
            ReplayProblem::StateMismatch => "replayMismatch",
        });
        rust_i18n::t!("replayAt", line = issue.record_no, reason = reason).into_owned()
    } else {
        tr(match e.kind() {
            io::ErrorKind::NotFound => "replayNotFound",
            io::ErrorKind::PermissionDenied => "replayDenied",
            _ => "replayReadFailure",
        })
    }
}

#[derive(Resource, Default)]
pub(super) struct SaveError(Option<String>);

#[derive(Component)]
pub(super) struct SettingsCard;

pub(super) fn install(app: &mut App) {
    let settings = path().map(|p| read(&p)).unwrap_or_default();
    settings.language.activate();
    app.insert_resource(settings).init_resource::<SaveError>();
}

pub(super) fn settings_contents(
    mut c: Commands,
    settings: Res<Preferences>,
    err: Res<SaveError>,
    card: Single<(Entity, Ref<SettingsCard>)>,
) {
    if !card.1.is_added() && !settings.is_changed() && !err.is_changed() {
        return;
    }

    c.entity(card.0).despawn_children().with_children(|p| {
        p.spawn(spread()).with_children(|p| {
            p.spawn(headline(&tr("settings"), 32.));
            p.spawn((
                compact(&tr("close")),
                observe(|_: On<Activate>, mut panel: ResMut<menu::Panel>| {
                    *panel = menu::Panel::Closing;
                }),
            ));
        });
        p.spawn(eyebrow(&tr("language")));
        p.spawn(row()).with_children(|p| {
            for (value, name) in [
                (Language::Chinese, "简体中文"),
                (Language::English, "English"),
            ] {
                p.spawn(preference(name, settings.language == value, move |s| {
                    s.language = value;
                    value.activate();
                }));
            }
        });
        if let Some(e) = &err.0 {
            p.spawn(label(format!("{}: {e}", tr("saveError")), 15., quiet()));
        }
    });
}

fn preference(
    name: &str,
    active: bool,
    set: impl Fn(&mut Preferences) + Send + Sync + 'static,
) -> impl Bundle {
    (
        choice(name, active),
        observe(
            move |_: On<Activate>,
                  mut settings: ResMut<Preferences>,
                  mut err: ResMut<SaveError>| {
                set(&mut settings);
                err.0 = path()
                    .and_then(|p| save(&p, &settings))
                    .err()
                    .map(|e| e.to_string());
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_settings_survive_restart_and_bad_file_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("用户 目录/settings.json");
        assert_eq!(read(&p), Preferences::default());
        let settings = Preferences {
            language: Language::English,
            ..Default::default()
        };
        save(&p, &settings).unwrap();

        assert_eq!(read(&p), settings);
        fs::write(&p, br#"{"language":"en","rate":"Fast"}"#).unwrap();

        assert_eq!(read(&p), settings);
        save(&p, &settings).unwrap();

        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&p).unwrap()).unwrap(),
            serde_json::json!({"language": "en"})
        );
        fs::write(&p, b"bad").unwrap();
        assert_eq!(read(&p), Preferences::default());
    }

    #[test]
    fn every_embedded_key_has_both_locales() {
        let table: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/ui.json")).unwrap();
        for (k, v) in table.as_object().unwrap() {
            if k == "_version" {
                continue;
            }

            for l in ["en", "zh-CN"] {
                assert_eq!(
                    rust_i18n::t!(k.as_str(), locale = l).as_ref(),
                    v[l].as_str().unwrap()
                );
            }
        }
    }
}
