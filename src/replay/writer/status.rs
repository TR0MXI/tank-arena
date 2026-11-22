use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Debug, thiserror::Error)]
pub enum SaveFailure {
    #[error("replay queue is full")]
    QueueFull,
    #[error("recording interrupted")]
    Interrupted,
    #[error("{0}")]
    Io(Arc<io::Error>),
}

impl From<io::Error> for SaveFailure {
    fn from(value: io::Error) -> Self {
        Self::Io(Arc::new(value))
    }
}

#[derive(Clone, Debug, Default)]
pub enum RecordStatus {
    #[default]
    Writing,
    Saved(PathBuf),
    Incomplete(SaveFailure),
}

#[derive(Default)]
pub(super) struct OutputStatus(Mutex<RecordStatus>);

impl OutputStatus {
    fn get(&self) -> RecordStatus {
        self.0.lock().unwrap().clone()
    }

    pub(super) fn finish(&self, result: Result<PathBuf, SaveFailure>) {
        let mut state = self.0.lock().unwrap();
        // 队列满是原始错误；之后写线程发现通道关闭，不可覆盖已向玩家报告的原因。
        if matches!(*state, RecordStatus::Writing) {
            *state = match result {
                Ok(path) => RecordStatus::Saved(path),
                Err(e) => RecordStatus::Incomplete(e),
            };
        }
    }
}

#[derive(Default)]
pub(super) struct Outputs {
    pub replay: OutputStatus,
    pub result: OutputStatus,
    pub queued: AtomicUsize,
    pub peak: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct Recording(pub(super) Arc<Outputs>);

impl Recording {
    pub fn high_water(&self) -> usize {
        self.0.peak.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> RecordStatus {
        self.0.replay.get()
    }

    pub fn result_status(&self) -> RecordStatus {
        self.0.result.get()
    }
}
