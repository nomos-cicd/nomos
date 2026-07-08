use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    job::JobResult,
    log::{Log, LogLevel},
    script::models::ScriptStatus,
    AppState,
};

#[derive(Deserialize)]
pub struct JobResultsQuery {
    #[serde(rename = "job-id")]
    job_id: Option<String>,
}

#[derive(Deserialize)]
pub struct LogsQuery {
    after: Option<u64>,
    before: Option<u64>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct LogItem {
    start_offset: u64,
    end_offset: u64,
    timestamp: DateTime<Utc>,
    level: LogLevel,
    message: String,
    step_name: String,
}

#[derive(Serialize)]
pub struct LogsResponse {
    logs: Vec<LogItem>,
    has_older: bool,
    next_before: Option<u64>,
    next_after: Option<u64>,
    file_size: u64,
    status: ScriptStatus,
    finished: bool,
}

pub async fn get_job_results(query: Query<JobResultsQuery>) -> Response {
    match JobResult::get_all(query.job_id.clone()) {
        Ok(results) => Json(results).into_response(),
        Err(e) => {
            eprintln!("Failed to get job results: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn get_job_result(Path(id): Path<String>) -> Response {
    match JobResult::get(id.as_str()) {
        Ok(Some(result)) => Json(result).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(JobResult::create_dummy())).into_response(),
        Err(e) => {
            eprintln!("Failed to get job result {}: {}", id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(JobResult::create_dummy())).into_response()
        }
    }
}

pub async fn stop_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.job_executor.stop_job(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            eprintln!("Failed to stop job {}: {}", id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn get_job_result_logs(Path(id): Path<String>, Query(query): Query<LogsQuery>) -> Response {
    match JobResult::get(&id) {
        Ok(Some(result)) => {
            let logger = match result.logger.lock() {
                Ok(logger) => logger.clone(),
                Err(_) => {
                    eprintln!("Failed to lock logger for job result {}", id);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

            match logger.get_log_page(query.after, query.before, query.limit.unwrap_or(200)) {
                Ok(page) => {
                    let logs = page
                        .entries
                        .into_iter()
                        .map(|entry| {
                            let Log {
                                timestamp,
                                level,
                                message,
                                step_name,
                            } = entry.log;

                            LogItem {
                                start_offset: entry.start_offset,
                                end_offset: entry.end_offset,
                                timestamp,
                                level,
                                message,
                                step_name,
                            }
                        })
                        .collect();

                    Json(LogsResponse {
                        logs,
                        has_older: page.has_older,
                        next_before: page.next_before,
                        next_after: page.next_after,
                        file_size: page.file_size,
                        status: result.status,
                        finished: result.finished_at.is_some(),
                    })
                    .into_response()
                }
                Err(e) => {
                    eprintln!("Failed to get logs for job result {}: {}", id, e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Ok(None) => {
            eprintln!("Job result not found: {}", id);
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            eprintln!("Failed to get job result {}: {}", id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
