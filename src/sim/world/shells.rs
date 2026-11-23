use super::*;

/// 弹槽随炮弹一起借出和归还；命中、过期和连续反弹消失共用同一条回收路径。
#[derive(Clone, Debug)]
pub(super) struct Shells {
    rounds: Vec<Shell>,
    remaining: [u8; MAX_SLOTS],
}

impl Shells {
    pub(super) fn from_rounds(rounds: Vec<Shell>, items: &Items) -> Self {
        let mut remaining = [AMMO; MAX_SLOTS];
        for shell in rounds
            .iter()
            .filter(|s| !items.projectiles.contains_key(&s.id))
        {
            remaining[shell.owner.0 as usize - 1] -= 1;
        }
        Self { rounds, remaining }
    }

    pub(super) fn as_slice(&self) -> &[Shell] {
        &self.rounds
    }

    pub(super) fn ammo(&self, slot: SlotId) -> u8 {
        self.remaining[slot.0 as usize - 1]
    }

    pub(super) fn push(&mut self, shell: Shell) {
        self.remaining[shell.owner.0 as usize - 1] -= 1;
        self.rounds.push(shell);
    }

    pub(super) fn push_special(&mut self, shell: Shell) {
        self.rounds.push(shell);
    }

    pub(super) fn retain_mut(
        &mut self,
        meta: &mut std::collections::BTreeMap<u64, Payload>,
        mut survives: impl FnMut(&mut Shell, Option<&mut Payload>) -> bool,
    ) {
        self.rounds.retain_mut(|shell| {
            if survives(shell, meta.get_mut(&shell.id)) {
                return true;
            }
            if meta.remove(&shell.id).is_none() {
                self.remaining[shell.owner.0 as usize - 1] += 1;
            }
            false
        });
    }
}
