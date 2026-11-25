use std::{fs, path::Path, sync::Arc};
use tank_arena::sim::*;

pub fn world() -> World {
    World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        (1..=2)
            .map(|n| Seat {
                slot: SlotId(n),
                team: TeamId(n),
            })
            .collect(),
        ClockRules::default(),
    )
    .unwrap()
}
pub fn write(root: &Path, name: &str, text: &str) {
    let p = root.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
