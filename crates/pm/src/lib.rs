#![forbid(unsafe_code)]
//! The optional project manager layer (docs/05): the work board as data,
//! claims, conflict checks, generated briefs, structured reports, Markdown
//! export and the shipped PM skill.
//!
//! Nothing in the core depends on this crate (D-N6YH). InLoop holds the
//! PM's state; it doesn't start sessions or make the PM's judgements
//! (D-E2GZ). The PM is the only writer of the board (D-Y40K), and every
//! record is its own file with a stable ID so boards can merge when sync
//! arrives (D-V3ZM). Every write goes through the redactor (D-G1R6).

pub mod board;
pub mod brief;
pub mod check;
pub mod export;
pub mod model;
pub mod ops;
pub mod skill;

pub use board::{Board, BoardError, CorruptRecord, Listing};
pub use brief::{BriefContext, BriefDecision, DEFAULT_BRIEF, RenderedBrief, render_brief};
pub use check::{CheckError, CheckReport, Conflict, ConflictReason, board_check};
pub use export::export_markdown;
pub use model::{
    Blocker, BlockerKind, CheckResult, Claim, Note, Record, RecordKind, Report, Task, TaskStatus,
};
pub use ops::{
    NewBlocker, NewReport, NewTask, OpError, TaskUpdate, UpdateContext, UpdateOutcome,
    apply_blocker, apply_report, new_blocker, new_report, new_task, newly_unblocked, update_task,
};
pub use skill::{PM_SKILL, SKILL_NAME};
