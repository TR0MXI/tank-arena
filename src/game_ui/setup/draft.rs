use super::*;
use std::io;

pub(super) const MODELS_PER_PAGE: usize = 5;

pub(super) struct TankEntry {
    pub(super) model: Model,
    pub(super) team: TeamId,
}

#[derive(Default, PartialEq)]
pub(super) enum LimitMode {
    #[default]
    Default,
    Endless,
    Custom,
}

#[derive(Resource)]
pub(super) struct Draft {
    // 数组下标就是坦克编号减一；不重排编号，也不另设席位计数器。
    tanks: [Option<TankEntry>; MAX_SLOTS],
    // 内置车型用类型、脚本用路径标识，不依赖候选列表中的位置。
    scanned: bool,
    scan_error: Option<String>,
    pub(super) models: Vec<Model>,
    pub(super) model_page: usize,
    pub(super) selected_team: TeamId,
    pub(super) teams: Vec<TeamId>,
    pub(super) confirm_team: Option<TeamId>,
    pub(super) inspecting: Item,
    pub(super) dimensions: [usize; 2],
    pub(super) limit: LimitMode,
    pub(super) seconds: String,
    pub(super) note: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            tanks: Default::default(),
            models: Vec::new(),
            scanned: false,
            scan_error: None,
            model_page: 0,
            selected_team: TeamId(1),
            teams: vec![TeamId(1), TeamId(2)],
            confirm_team: None,
            inspecting: Item::Laser,
            dimensions: [12, 8],
            limit: LimitMode::Default,
            seconds: String::new(),
            note: String::new(),
        }
    }
}

impl Draft {
    /// 扫描只更新车型身份；文件预览和正式出战的脚本快照分别加载。
    /// 轮询无变化时返回 false，避免重建面板或中断正在进行的拖动。
    fn reconcile(&mut self, scan: io::Result<Vec<Model>>) -> bool {
        let (models, scan_error) = match scan {
            Ok(models) => (models, None),
            Err(e) => {
                let error = Some(e.to_string());
                if self.scanned {
                    if self.scan_error == error {
                        return false;
                    }
                    self.scan_error = error;
                    return true; // 保留已有阵容，不把暂时读不到的文件当作已删除。
                }
                (vec![Model::Avalon], error) // 首次读取失败也能玩内置车型。
            }
        };
        let error_changed = self.scan_error != scan_error;
        self.scan_error = scan_error;
        if self.scanned && self.models == models {
            return error_changed;
        }

        if !self.scanned {
            // 没有本地脚本时，默认两队各用一辆原生 Avalon。
            for (i, model) in models.iter().cycle().take(2).enumerate() {
                self.tanks[i] = Some(TankEntry {
                    model: model.clone(),
                    team: TeamId(i as u32 + 1),
                });
            }

            self.scanned = true;
        } else {
            for slot in &mut self.tanks {
                if slot
                    .as_ref()
                    .is_some_and(|t| models.binary_search(&t.model).is_err())
                {
                    *slot = None;
                }
            }
        }
        self.model_page = self
            .model_page
            .min(models.len().saturating_sub(1) / MODELS_PER_PAGE);
        self.models = models;
        true
    }

    pub(super) fn visible_models(&self) -> &[Model] {
        self.models
            .chunks(MODELS_PER_PAGE)
            .nth(self.model_page)
            .unwrap_or_default()
    }

    pub(super) fn slots(&self) -> impl Iterator<Item = (SlotId, &TankEntry)> {
        self.tanks
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.as_ref().map(|t| (SlotId(i as u32 + 1), t)))
    }

    pub(super) fn tank(&self, slot: SlotId) -> Option<&TankEntry> {
        self.tanks.get(slot.0.checked_sub(1)? as usize)?.as_ref()
    }

    pub(super) fn deploy(&mut self, model: &Model, team: TeamId) {
        if self.models.binary_search(model).is_ok()
            && self.teams.contains(&team)
            && let Some(slot) = self.tanks.iter_mut().find(|t| t.is_none())
        {
            *slot = Some(TankEntry {
                model: model.to_owned(),
                team,
            });
            self.select_team(team);
        }
    }

    pub(super) fn remove(&mut self, slot: SlotId) {
        self.tanks[(slot.0 - 1) as usize] = None;
    }

    pub(super) fn select_team(&mut self, team: TeamId) {
        self.selected_team = team;
        self.confirm_team = None;
    }

    pub(super) fn add_team(&mut self) {
        // 复用空闲的颜色或编号，不改动现有队伍或席位。
        if let Some(team) = (1..=MAX_SLOTS as u32)
            .map(TeamId)
            .find(|id| !self.teams.contains(id))
        {
            self.teams.push(team);
            self.select_team(team);
        }
    }

    pub(super) fn request_remove_team(&mut self, team: TeamId) {
        if self.teams.len() <= 1 || !self.teams.contains(&team) {
            return;
        }

        if self.seats().any(|s| s.team == team) {
            self.confirm_team = Some(team);
        } else {
            self.discard_team(team);
        }
    }

    pub(super) fn confirm_remove_team(&mut self, team: TeamId) {
        if self.confirm_team == Some(team) {
            self.discard_team(team);
        }
    }

    fn discard_team(&mut self, team: TeamId) {
        let Some(index) = self.teams.iter().position(|&id| id == team) else {
            return;
        };

        self.teams.remove(index);
        for slot in &mut self.tanks {
            if slot.as_ref().is_some_and(|tank| tank.team == team) {
                *slot = None;
            }
        }

        if self.selected_team == team {
            self.selected_team = self.teams[index.min(self.teams.len() - 1)];
        }
        self.confirm_team = None;
    }

    pub(super) fn seats(&self) -> impl Iterator<Item = Seat> {
        self.slots().map(|(slot, t)| Seat { slot, team: t.team })
    }

    pub(super) fn clock(&self) -> Result<ClockRules, String> {
        match self.limit {
            LimitMode::Default => Ok(ClockRules::default()),
            LimitMode::Endless => Ok(ClockRules::unlimited()),
            LimitMode::Custom => self.seconds.parse().map_err(config_error),
        }
    }

    pub(super) fn config(&self, map: &ArenaMap) -> Result<(ClockRules, Vec<Seat>), String> {
        if let Some(error) = &self.scan_error
            && self.slots().any(|(_, t)| matches!(t.model, Model::File(_)))
        {
            return Err(format!("{}: {error}", tr("scanError")));
        }
        let clock = self.clock()?;
        let roster = Roster::new(self.seats().collect(), map).map_err(config_error)?;
        Ok((clock, roster.seats().to_vec()))
    }
}

pub(super) fn sync_models(d: &mut ResMut<Draft>) {
    let scan = local_files::bots_dir()
        .and_then(|dir| tank_arena::bot::catalog::discover(&dir))
        .map(Model::lineup);
    if d.bypass_change_detection().reconcile(scan) {
        d.set_changed();
    }
}

pub(super) fn refresh_models(mut d: ResMut<Draft>) {
    sync_models(&mut d);
}

pub(super) fn config_error(e: ConfigError) -> String {
    tr(match e {
        ConfigError::SlotCount => "invalidCount",
        ConfigError::SlotRange => "invalidNumber",
        ConfigError::Duplicate => "invalidDuplicate",
        ConfigError::Teams => "invalidTeams",
        ConfigError::Spawns => "invalidSpawns",
        ConfigError::Seconds => "invalidSeconds",
        ConfigError::Overflow => "invalidOverflow",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_is_playable_without_files_and_a_namesake_stays_a_separate_pilot() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Draft::default();
        let scan = || tank_arena::bot::catalog::discover(dir.path()).map(Model::lineup);
        let map = SWITCHYARD.parse().unwrap();
        d.reconcile(scan());
        let (_, seats) = d.config(&map).unwrap();
        assert_eq!(seats.len(), 2);
        assert!(d.slots().all(|(_, t)| t.model == Model::Avalon));
        let native = d.tank(seats[0].slot).unwrap().model.entrant(seats[0]);
        assert!(matches!(
            native.pilot,
            Ok(tank_arena::runner::Pilot::Avalon)
        ));
        // Even an unreadable scripts directory is not a dependency of a native mirror.
        let mut unreadable = Draft::default();
        unreadable.reconcile(Err(io::Error::other("cannot scan scripts")));
        assert_eq!(unreadable.config(&map).unwrap().1, seats);
        let path = dir.path().join("Avalon.luau");
        std::fs::write(&path, "return function() end").unwrap();
        let local = Model::File(path.clone());
        d.reconcile(scan());
        d.deploy(&local, TeamId(1));
        let seat = d.seats().last().unwrap();
        let old = d.tank(seat.slot).unwrap().model.entrant(seat);
        assert!(old.script().is_some());
        assert_ne!(native.name, old.name);
        assert_ne!(native.fingerprint(), old.fingerprint());
        std::fs::remove_file(path).unwrap();
        d.reconcile(scan());
        assert_eq!(d.models, [Model::Avalon]);
        assert_eq!(d.config(&map).unwrap().1, seats);
    }

    #[test]
    fn one_model_seeds_a_playable_mirror_match() {
        let mut draft = Draft::default();
        let model = Model::File("example.luau".into());
        draft.reconcile(Ok(vec![model.clone()]));
        let (_, seats) = draft.config(&SWITCHYARD.parse().unwrap()).unwrap();
        assert_eq!(seats.len(), 2);
        assert_ne!(seats[0].team, seats[1].team);
        assert!(
            seats
                .iter()
                .all(|s| draft.tank(s.slot).unwrap().model == model)
        );
    }

    #[test]
    fn models_are_reusable_across_teams_and_only_deployments_take_slots() {
        let mut d = Draft::default();
        let model = &Model::File("example.luau".into());
        d.reconcile(Ok(vec![]));
        let mut paths: Vec<_> = (0..12)
            .map(|i| Model::File(format!("model{i}").into()))
            .collect();
        paths.push(model.to_owned());
        paths.sort();
        d.reconcile(Ok(paths));

        assert_eq!((d.models.len(), d.seats().count()), (13, 0));
        for i in 0..MAX_SLOTS + 1 {
            d.deploy(model, TeamId(1 + (i % 2) as u32));
        }

        assert_eq!(d.seats().count(), MAX_SLOTS);
        d.remove(SlotId(3));

        assert!(d.tank(SlotId(3)).is_none());
        assert_eq!(d.tank(SlotId(4)).unwrap().model, *model);
        let replacement = &Model::File("model11".into());
        d.deploy(replacement, TeamId(2));
        let (_, seats) = d.config(&SWITCHYARD.parse().unwrap()).unwrap();
        assert_eq!(seats[2].slot, SlotId(3));
        assert_eq!(seats[2].team, TeamId(2));
        assert_eq!(d.tank(seats[2].slot).unwrap().model, *replacement);
        assert!(d.models.contains(&model.to_owned()));
    }

    #[test]
    fn live_catalog_preserves_path_identity_and_reloads_scripts_at_launch() {
        use std::fs;
        use tank_arena::bot::catalog::discover;

        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let beta = dir.join("beta.luau");
        let gamma = dir.join("gamma/bot.luau");
        fs::create_dir(dir.join("gamma")).unwrap();
        fs::write(&beta, "old").unwrap();
        fs::write(&gamma, "other").unwrap();
        let mut d = Draft::default();
        let scan = |d: &mut Draft| {
            d.reconcile(discover(dir).map(|ps| ps.into_iter().map(Model::File).collect()))
        };
        assert!(scan(&mut d));
        d.deploy(&Model::File(gamma.clone()), TeamId(1));
        let first = d.seats().next().unwrap();
        let entrant = |d: &Draft| d.tank(first.slot).unwrap().model.entrant(first);
        let playing = entrant(&d);

        // 即使在已部署路径之前插入新条目，也不能换掉任何席位的脚本。
        for i in 0..5 {
            fs::write(dir.join(format!("a{i}.luau")), "extra").unwrap();
        }

        assert!(scan(&mut d));
        d.model_page = 1;

        assert_eq!(d.tank(SlotId(1)).unwrap().model, Model::File(beta.clone()));
        assert_eq!(d.tank(SlotId(2)).unwrap().model, Model::File(gamma.clone()));

        // 同名文件被原子替换时，无须重建阵容或重启程序。
        fs::write(dir.join("save.tmp"), "new").unwrap();
        fs::rename(dir.join("save.tmp"), &beta).unwrap();

        assert!(!scan(&mut d));
        assert_eq!(playing.script().unwrap().text(), "old");
        assert_eq!(entrant(&d).script().unwrap().text(), "new");
        fs::remove_file(&gamma).unwrap();
        for i in 0..5 {
            fs::remove_file(dir.join(format!("a{i}.luau"))).unwrap();
        }

        assert!(scan(&mut d));
        assert_eq!(d.model_page, 0);
        d.deploy(&Model::File(gamma.clone()), TeamId(2)); // 过期卡片不能部署成另一个模型。

        assert_eq!(d.seats().collect::<Vec<_>>(), vec![first]);
        fs::write(&gamma, "returned").unwrap();

        assert!(scan(&mut d));
        assert_eq!(d.seats().count(), 1); // 重新出现的模型只回到候选列表，不自动参赛。
    }

    #[test]
    fn a_scan_error_recovers_without_clearing_the_lineup() {
        let mut d = Draft::default();
        let paths = vec![Model::File("a.luau".into()), Model::File("b.luau".into())];
        let map = SWITCHYARD.parse().unwrap();
        d.reconcile(Ok(paths.clone()));

        assert!(d.reconcile(Err(io::Error::other("temporary"))));
        assert!(!d.reconcile(Err(io::Error::other("temporary"))));
        assert_eq!(d.seats().count(), 2);
        assert!(d.config(&map).is_err());
        assert!(d.reconcile(Ok(paths)));
        assert!(d.config(&map).is_ok());
    }

    #[test]
    fn removing_an_empty_team_repairs_selection_and_keeps_launch_validation() {
        let mut d = Draft::default();
        let map = SWITCHYARD.parse().unwrap();
        let model = &Model::File("A".into());
        d.reconcile(Ok(vec![model.to_owned()]));
        d.remove(SlotId(2)); // 本用例从只有一队已部署的局面开始。
        d.add_team();

        assert_eq!(d.selected_team, TeamId(3));
        d.deploy(model, d.selected_team);
        d.request_remove_team(TeamId(3));

        assert_eq!(d.teams.len(), 3);
        assert_eq!(d.confirm_team, Some(TeamId(3)));
        d.remove(SlotId(2));
        d.request_remove_team(TeamId(3));

        assert_eq!(d.selected_team, TeamId(2));
        d.deploy(model, TeamId(3));

        assert!(d.config(&map).is_err());
        d.deploy(model, d.selected_team);
        d.limit = LimitMode::Custom;
        for s in ["", "0", "-20", "9", "10.5", "99999999999999999999"] {
            d.seconds = s.into();
            assert!(d.config(&map).is_err());
        }

        d.seconds = "75".into();
        assert_eq!(d.config(&map).unwrap().0.seconds(), Some(75));
    }

    #[test]
    fn deleting_a_populated_middle_team_requires_confirmation_and_preserves_other_ids() {
        let mut d = Draft::default();
        let model = &Model::File("one.luau".into());
        d.reconcile(Ok(vec![model.to_owned()]));
        d.add_team();
        d.deploy(model, TeamId(3));
        d.request_remove_team(TeamId(2));
        d.request_remove_team(TeamId(2)); // 重复触发旧按钮不算确认。

        assert_eq!(d.seats().count(), 3);
        d.confirm_remove_team(TeamId(1));

        assert_eq!(d.seats().count(), 3);
        d.select_team(TeamId(2)); // 取消待执行的操作。
        d.confirm_remove_team(TeamId(2));

        assert_eq!(d.seats().count(), 3);
        d.request_remove_team(TeamId(2));
        d.confirm_remove_team(TeamId(2));

        assert_eq!(d.teams, [TeamId(1), TeamId(3)]);
        assert_eq!(d.selected_team, TeamId(3));
        assert!(d.tank(SlotId(2)).is_none());
        assert_eq!(d.tank(SlotId(3)).unwrap().team, TeamId(3));
        assert_eq!(d.models, std::slice::from_ref(model));
        d.add_team();

        assert_eq!(d.teams, [TeamId(1), TeamId(3), TeamId(2)]);
        for _ in 0..MAX_SLOTS {
            d.add_team();
        }

        assert_eq!(d.teams.len(), MAX_SLOTS);
        assert_eq!(
            d.teams
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            MAX_SLOTS
        );
    }
}
