use super::{Entrant, Script, Seat, tank_manager::Looks};
use std::path::PathBuf;
use tank_arena::appearance::PixelSkin;

/// A built-in has no filesystem identity: a local Avalon.luau can never shadow it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Model {
    Avalon,
    File(PathBuf),
}

impl Model {
    pub(super) fn lineup(sorted_files: Vec<PathBuf>) -> Vec<Self> {
        std::iter::once(Self::Avalon)
            .chain(sorted_files.into_iter().map(Self::File))
            .collect()
    }

    pub(super) fn entrant(&self, seat: Seat) -> Entrant {
        match self {
            Self::Avalon => Entrant::avalon(seat),
            Self::File(path) => Entrant::from_file(seat, path),
        }
    }

    pub(super) fn skin(&self, looks: &Looks, script: Option<&Script>) -> Option<PixelSkin> {
        match self {
            Self::Avalon => Some(
                PixelSkin::parse(include_str!("../../../assets/skins/avalon.txt"))
                    .expect("bundled Avalon livery"),
            ),
            Self::File(path) => looks.resolve(path, script.map(Script::text)),
        }
    }
}
