use super::*;

/// 弹槽随炮弹一起借出和归还；命中、过期和连续反弹消失共用同一条回收路径。
#[derive(Clone, Debug)]
pub(super) struct Shells {
    rounds: Vec<Shell>,
    remaining: [u8; MAX_SLOTS],
}

impl Shells {
    pub(super) fn from_rounds(rounds: Vec<Shell>) -> Self {
        let mut remaining = [AMMO; MAX_SLOTS];
        for shell in &rounds {
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

    pub(super) fn retain_mut(&mut self, mut survives: impl FnMut(&mut Shell) -> bool) {
        self.rounds.retain_mut(|shell| {
            if survives(shell) {
                return true;
            }
            self.remaining[shell.owner.0 as usize - 1] += 1;
            false
        });
    }
}
