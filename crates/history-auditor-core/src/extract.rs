use super::schema::ActionEvent;
use super::schema::ErrorEvent;
use super::schema::IntentEvent;
use super::schema::TaskRun;
#[derive(Clone, Debug)]
pub struct AuditExtraction {
    pub intents: Vec<IntentEvent>,
    pub actions: Vec<ActionEvent>,
    pub errors: Vec<ErrorEvent>,
    pub task_run: TaskRun,
}
fn shellish_command(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("$ ").map(str::to_string))
}
