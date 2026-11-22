use std::{fmt, io};

/// 回放格式与界面语言无关。保留结构化原因和记录序号供界面翻译，底层诊断仍可供开发者定位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayProblem {
    Format,
    Version,
    Configuration,
    Truncated,
    Sequence,
    StateMismatch,
}

#[derive(Debug, thiserror::Error)]
#[error("record {record_no}: {detail}")]
pub struct ReplayIssue {
    pub record_no: u64,
    pub problem: ReplayProblem,
    detail: String,
}

impl ReplayIssue {
    pub(super) fn decoding(record_no: u64, error: io::Error) -> io::Error {
        if error.get_ref().is_some_and(|e| e.is::<Self>()) {
            return error;
        }

        let problem = if error.kind() == io::ErrorKind::UnexpectedEof {
            ReplayProblem::Truncated
        } else {
            ReplayProblem::Format
        };
        Self::at(record_no, problem, error)
    }

    pub(super) fn at(
        record_no: u64,
        problem: ReplayProblem,
        detail: impl fmt::Display,
    ) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            Self {
                record_no,
                problem,
                detail: detail.to_string(),
            },
        )
    }
}
