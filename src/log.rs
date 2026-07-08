use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const REVERSE_READ_CHUNK_SIZE: u64 = 64 * 1024;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARNING"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Log {
    pub level: LogLevel,
    pub message: String,
    pub step_name: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JobLogger {
    log_filename: PathBuf,
    job_id: String,
    result_id: String,
}

impl JobLogger {
    pub fn new(job_id: String, result_id: String, dry_run: bool) -> Result<Self, String> {
        let log_path = primary_log_file_path(&result_id)?;
        if dry_run {
            return Ok(JobLogger {
                log_filename: log_path.clone(),
                job_id,
                result_id,
            });
        }

        // Create directory if it doesn't exist
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path.clone())
            .map_err(|e| e.to_string())?;

        Ok(JobLogger {
            log_filename: log_path.clone(),
            job_id,
            result_id,
        })
    }

    pub fn log(&mut self, level: LogLevel, step_name: &str, message: &str) -> Result<(), String> {
        let log = Log {
            level,
            message: message.to_string(),
            step_name: step_name.to_string(),
            timestamp: Utc::now(),
        };

        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.log_filename)
            .map_err(|e| e.to_string())?;

        writeln!(file, "{}", serde_json::to_string(&log).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn get_logs(&self) -> Result<Vec<Log>, String> {
        let path = readable_log_file_path(&self.result_id)?;
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };

        let logs = content
            .lines()
            .filter_map(|line| serde_json::from_str::<Log>(line).ok())
            .collect();

        Ok(logs)
    }

    pub fn get_log_page(&self, after: Option<u64>, before: Option<u64>, limit: usize) -> Result<LogPage, String> {
        let limit = limit.clamp(1, 500);
        let path = readable_log_file_path(&self.result_id)?;
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LogPage {
                    entries: Vec::new(),
                    has_older: false,
                    next_before: None,
                    next_after: Some(0),
                    file_size: 0,
                });
            }
            Err(e) => return Err(e.to_string()),
        };
        let file_size = file.metadata().map_err(|e| e.to_string())?.len();

        let entries = if let Some(after) = after {
            read_logs_after(&mut file, after.min(file_size), limit)?
        } else if let Some(before) = before {
            read_logs_before(&mut file, before.min(file_size), limit)?
        } else {
            read_logs_before(&mut file, file_size, limit)?
        };
        let first_offset = entries.first().map(|entry| entry.start_offset);
        let last_offset = entries.last().map(|entry| entry.end_offset);

        Ok(LogPage {
            entries,
            has_older: first_offset.is_some_and(|offset| offset > 0),
            next_before: first_offset.filter(|offset| *offset > 0),
            next_after: Some(last_offset.unwrap_or(file_size)),
            file_size,
        })
    }
}

pub struct LogPageEntry {
    pub start_offset: u64,
    pub end_offset: u64,
    pub log: Log,
}

pub struct LogPage {
    pub entries: Vec<LogPageEntry>,
    pub has_older: bool,
    pub next_before: Option<u64>,
    pub next_after: Option<u64>,
    pub file_size: u64,
}

fn read_logs_after(file: &mut std::fs::File, offset: u64, limit: usize) -> Result<Vec<LogPageEntry>, String> {
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut entries = Vec::with_capacity(limit);
    let mut position = offset;
    let mut line = String::new();

    while entries.len() < limit {
        line.clear();
        let bytes_read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if bytes_read == 0 {
            break;
        }

        let start_offset = position;
        position += bytes_read as u64;

        if let Some(entry) = parse_log_entry(start_offset, position, &line) {
            entries.push(entry);
        }
    }

    Ok(entries)
}

fn read_logs_before(file: &mut std::fs::File, before: u64, limit: usize) -> Result<Vec<LogPageEntry>, String> {
    let mut cursor = before;
    let mut buffer = Vec::new();

    while cursor > 0 {
        let chunk_start = cursor.saturating_sub(REVERSE_READ_CHUNK_SIZE);
        let chunk_len = (cursor - chunk_start) as usize;
        let mut chunk = vec![0; chunk_len];
        file.seek(SeekFrom::Start(chunk_start)).map_err(|e| e.to_string())?;
        file.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        chunk.extend_from_slice(&buffer);
        buffer = chunk;
        cursor = chunk_start;

        if count_lines(&buffer) > limit {
            break;
        }
    }

    let selected = line_ranges(&buffer, cursor)
        .into_iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>();

    let mut entries = selected
        .into_iter()
        .rev()
        .filter_map(|range| parse_log_entry(range.start_offset, range.end_offset, range.line))
        .collect::<Vec<_>>();
    entries.shrink_to_fit();
    Ok(entries)
}

fn count_lines(buffer: &[u8]) -> usize {
    let newline_count = buffer.iter().filter(|byte| **byte == b'\n').count();
    if buffer.last().is_some_and(|byte| *byte == b'\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

struct LineRange<'a> {
    start_offset: u64,
    end_offset: u64,
    line: &'a str,
}

fn line_ranges(buffer: &[u8], base_offset: u64) -> Vec<LineRange<'_>> {
    let mut ranges = Vec::new();
    let mut start = 0;

    for (index, byte) in buffer.iter().enumerate() {
        if *byte == b'\n' {
            if index > start {
                if let Ok(line) = std::str::from_utf8(&buffer[start..index]) {
                    ranges.push(LineRange {
                        start_offset: base_offset + start as u64,
                        end_offset: base_offset + index as u64 + 1,
                        line,
                    });
                }
            }
            start = index + 1;
        }
    }

    if start < buffer.len() {
        if let Ok(line) = std::str::from_utf8(&buffer[start..]) {
            ranges.push(LineRange {
                start_offset: base_offset + start as u64,
                end_offset: base_offset + buffer.len() as u64,
                line,
            });
        }
    }

    ranges
}

fn parse_log_entry(start_offset: u64, end_offset: u64, line: &str) -> Option<LogPageEntry> {
    serde_json::from_str::<Log>(line.trim_end_matches(['\r', '\n']))
        .ok()
        .map(|log| LogPageEntry {
            start_offset,
            end_offset,
            log,
        })
}

fn readable_log_file_path(result_id: &str) -> Result<PathBuf, String> {
    let primary = primary_log_file_path(result_id)?;
    if primary.exists() {
        return Ok(primary);
    }

    let legacy = legacy_log_file_path(result_id)?;
    if legacy.exists() {
        return Ok(legacy);
    }

    Ok(primary)
}

fn primary_log_file_path(result_id: &str) -> Result<PathBuf, String> {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").map_err(|e| e.to_string())?;
        Ok(PathBuf::from(appdata)
            .join("nomos")
            .join("job_results")
            .join(result_id)
            .join("log"))
    } else {
        Ok(PathBuf::from("/var/lib/nomos")
            .join("job_results")
            .join(result_id)
            .join("log"))
    }
}

fn legacy_log_file_path(result_id: &str) -> Result<PathBuf, String> {
    if cfg!(target_os = "windows") {
        primary_log_file_path(result_id)
    } else {
        Ok(PathBuf::from("/var/log/nomos")
            .join("job_results")
            .join(result_id)
            .join("log"))
    }
}
