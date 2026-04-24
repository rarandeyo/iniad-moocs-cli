pub mod common;

mod assignment_confirm;
mod assignment_drafts;
mod assignment_push;
mod completion;
#[cfg(target_os = "linux")]
mod destructive;
mod doctor_diagnostics;
mod global_options;
#[cfg(target_os = "linux")]
mod lesson_best_effort;
mod plumbing;
#[cfg(target_os = "linux")]
mod pty_probe;
mod walking_skeleton;
