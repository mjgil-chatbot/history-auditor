use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::schema::{RawActor, RawEvent, RawEventKind};
use super::session::SessionSnapshot;

pub fn ingest_chat_session(
    session: &SessionSnapshot,
    provider_session_path: &str,
) -> Result<Vec<RawEvent>> {
    ingest_provider_session(&session.id, session, provider_session_path)
}

pub fn ingest_provider_session(
    session_id: &str,
    session: &SessionSnapshot,
    provider_session_path: &str,
) -> Result<Vec<RawEvent>> {
    let mut events = Vec::new();
    for (index, message) in session.messages.iter().enumerate() {
        let actor = if message.is_user {
            RawActor::User
        } else {
            RawActor::Assistant
        };
        let span = format!("message[{index}]");
        let content = message.content.trim().to_string();
        events.push(build_raw_event(
            session_id,
            events.len(),
            actor,
            RawEventKind::Message,
            Some(message.timestamp),
            provider_session_path.to_string(),
            Some(span),
            content.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        events.extend(parse_shell_like_blocks(
            session_id,
            &content,
            provider_session_path,
            Some(message.timestamp),
            &format!("message[{index}]"),
            events.len(),
        ));
    }
    Ok(events)
}

pub fn ingest_jsonl_chat(session_id: &str, source_uri: &str, raw: &str) -> Result<Vec<RawEvent>> {
    let mut events = Vec::new();
    for (line_index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("failed to parse JSONL line {}", line_index + 1))?;
        let actor = actor_from_value(value.get("actor").and_then(Value::as_str));
        let kind = kind_from_value(value.get("kind").and_then(Value::as_str));
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));
        let content = value
            .get("content")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let command = value
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string);
        let stdout = value
            .get("stdout")
            .and_then(Value::as_str)
            .map(str::to_string);
        let stderr = value
            .get("stderr")
            .and_then(Value::as_str)
            .map(str::to_string);
        let exit_code = value
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok());
        events.push(build_raw_event(
            session_id,
            events.len(),
            actor,
            kind,
            timestamp,
            source_uri.to_string(),
            Some(format!("line:{}", line_index + 1)),
            content,
            None,
            None,
            command,
            None,
            stdout,
            stderr,
            exit_code,
        ));
    }
    Ok(events)
}

pub fn ingest_markdown_chat(
    session_id: &str,
    source_uri: &str,
    raw: &str,
) -> Result<Vec<RawEvent>> {
    let mut events = Vec::new();
    let mut actor = RawActor::External;
    let mut buffer = Vec::new();
    let mut start_line = 1_usize;

    for (line_index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        let next_actor = if trimmed.eq_ignore_ascii_case("user:")
            || trimmed.starts_with("## User")
            || trimmed.starts_with("# User")
        {
            Some(RawActor::User)
        } else if trimmed.eq_ignore_ascii_case("assistant:")
            || trimmed.starts_with("## Assistant")
            || trimmed.starts_with("# Assistant")
        {
            Some(RawActor::Assistant)
        } else {
            None
        };

        if let Some(next_actor) = next_actor {
            flush_markdown_event(
                session_id,
                source_uri,
                &mut events,
                &mut buffer,
                actor.clone(),
                start_line,
                line_index,
            );
            actor = next_actor;
            start_line = line_index + 2;
            continue;
        }
        buffer.push(line);
    }
    flush_markdown_event(
        session_id,
        source_uri,
        &mut events,
        &mut buffer,
        actor,
        start_line,
        raw.lines().count().max(start_line),
    );
    Ok(events)
}

pub fn ingest_shell_log(session_id: &str, source_uri: &str, raw: &str) -> Result<Vec<RawEvent>> {
    let mut events = Vec::new();
    events.extend(parse_shell_like_blocks(
        session_id,
        raw,
        source_uri,
        None,
        "shell_log",
        0,
    ));
    Ok(events)
}

fn flush_markdown_event(
    session_id: &str,
    source_uri: &str,
    events: &mut Vec<RawEvent>,
    buffer: &mut Vec<&str>,
    actor: RawActor,
    start_line: usize,
    end_line: usize,
) {
    if buffer.is_empty() {
        return;
    }
    let content = buffer.join("\n").trim().to_string();
    buffer.clear();
    if content.is_empty() {
        return;
    }
    events.push(build_raw_event(
        session_id,
        events.len(),
        actor,
        RawEventKind::Message,
        None,
        source_uri.to_string(),
        Some(format!("lines:{start_line}-{end_line}")),
        content,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));
}

fn parse_shell_like_blocks(
    session_id: &str,
    raw: &str,
    source_uri: &str,
    timestamp: Option<DateTime<Utc>>,
    source_prefix: &str,
    start_ordinal: usize,
) -> Vec<RawEvent> {
    let mut events = Vec::new();
    let mut current_command: Option<(usize, String, Vec<String>)> = None;

    for (line_index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        let command = trimmed
            .strip_prefix("$ ")
            .or_else(|| trimmed.strip_prefix("> "))
            .or_else(|| trimmed.strip_prefix("command: "))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        if let Some(command) = command {
            if let Some((command_line, prior_command, output)) = current_command.take() {
                push_shell_pair(
                    session_id,
                    source_uri,
                    timestamp,
                    source_prefix,
                    start_ordinal + events.len(),
                    command_line,
                    prior_command,
                    output,
                    &mut events,
                );
            }
            current_command = Some((line_index + 1, command, Vec::new()));
            continue;
        }

        if let Some((_, _, output)) = current_command.as_mut() {
            output.push(line.to_string());
        }
    }

    if let Some((command_line, command, output)) = current_command.take() {
        push_shell_pair(
            session_id,
            source_uri,
            timestamp,
            source_prefix,
            start_ordinal + events.len(),
            command_line,
            command,
            output,
            &mut events,
        );
    }

    events
}

fn push_shell_pair(
    session_id: &str,
    source_uri: &str,
    timestamp: Option<DateTime<Utc>>,
    source_prefix: &str,
    ordinal_seed: usize,
    command_line: usize,
    command: String,
    output: Vec<String>,
    events: &mut Vec<RawEvent>,
) {
    let output_text = output.join("\n");
    let exit_code = parse_exit_code(&output_text);
    let stderr = parse_tagged_block(&output_text, "stderr");
    let stdout = parse_tagged_block(&output_text, "stdout");

    events.push(build_raw_event(
        session_id,
        ordinal_seed,
        RawActor::Tool,
        RawEventKind::ShellCommand,
        timestamp,
        source_uri.to_string(),
        Some(format!("{source_prefix}:line:{command_line}")),
        command.clone(),
        None,
        None,
        Some(command),
        None,
        None,
        None,
        None,
    ));
    events.push(build_raw_event(
        session_id,
        ordinal_seed + 1,
        RawActor::Tool,
        RawEventKind::ShellResult,
        timestamp,
        source_uri.to_string(),
        Some(format!("{source_prefix}:line:{command_line}")),
        output_text,
        None,
        None,
        None,
        None,
        stdout,
        stderr,
        exit_code,
    ));
}

fn build_raw_event(
    session_id: &str,
    ordinal: usize,
    actor: RawActor,
    kind: RawEventKind,
    timestamp: Option<DateTime<Utc>>,
    source_uri: String,
    source_span: Option<String>,
    raw_content: String,
    tool_name: Option<String>,
    tool_args_json: Option<String>,
    command: Option<String>,
    cwd: Option<String>,
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: Option<i32>,
) -> RawEvent {
    let event_id = format!("e{:04}", ordinal + 1);
    let content_hash = hash_content(&(
        session_id,
        ordinal,
        &raw_content,
        &command,
        &stdout,
        &stderr,
    ));
    RawEvent {
        event_id,
        session_id: session_id.to_string(),
        ordinal,
        actor,
        kind,
        timestamp,
        source_uri,
        source_span,
        raw_content,
        tool_name,
        tool_args_json,
        command,
        cwd,
        stdout,
        stderr,
        exit_code,
        content_hash,
    }
}

fn parse_exit_code(text: &str) -> Option<i32> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(code) = trimmed
            .strip_prefix("[exit_code=")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|value| value.parse::<i32>().ok())
        {
            return Some(code);
        }
        if let Some(code) = trimmed
            .strip_prefix("exit_code=")
            .and_then(|value| value.parse::<i32>().ok())
        {
            return Some(code);
        }
    }
    None
}

fn parse_tagged_block(text: &str, tag: &str) -> Option<String> {
    let needle = format!("[{tag}]");
    let mut collecting = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == needle {
            collecting = true;
            continue;
        }
        if collecting && trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if collecting {
            lines.push(line);
        }
    }
    let value = lines.join("\n").trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn hash_content<T: Hash>(value: &T) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn actor_from_value(actor: Option<&str>) -> RawActor {
    match actor
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "user" => RawActor::User,
        "assistant" => RawActor::Assistant,
        "tool" => RawActor::Tool,
        "system" => RawActor::System,
        _ => RawActor::External,
    }
}

fn kind_from_value(kind: Option<&str>) -> RawEventKind {
    match kind
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "tool_call" => RawEventKind::ToolCall,
        "tool_result" => RawEventKind::ToolResult,
        "shell_command" => RawEventKind::ShellCommand,
        "shell_result" => RawEventKind::ShellResult,
        "file_change" => RawEventKind::FileChange,
        "error" => RawEventKind::Error,
        "summary" => RawEventKind::Summary,
        "artifact" => RawEventKind::Artifact,
        _ => RawEventKind::Message,
    }
}
