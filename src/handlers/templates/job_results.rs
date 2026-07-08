use askama::Template;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{job::JobResult, log::LogLevel};

#[derive(Template)]
#[template(path = "job-results.html")]
pub struct JobResultsTemplate<'a> {
    title: String,
    has_in_progress: bool,
    job_id_filter: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "job-results-table.html")]
pub struct JobResultsTableTemplate {
    results: Vec<JobResult>,
}

#[derive(Template)]
#[template(path = "job-result.html")]
pub struct JobResultTemplate<'a> {
    title: &'a str,
    result: &'a JobResult,
}

#[derive(Template)]
#[template(path = "job-result-logs.html")]
pub struct JobResultLogsTemplate<'a> {
    logs: Vec<FormattedLog<'a>>,
}

pub struct FormattedLog<'a> {
    pub timestamp: &'a DateTime<Utc>,
    pub level: &'a LogLevel,
    pub message: &'a str,
}

#[derive(Deserialize)]
pub struct JobResultsQuery {
    #[serde(rename = "job-id")]
    job_id: Option<String>,
}

#[derive(Template)]
#[template(path = "job-result-header.html")]
pub struct JobResultHeaderTemplate<'a> {
    result: &'a JobResult,
    running_for: String,
}

#[derive(Template)]
#[template(path = "job-result-steps.html")]
pub struct JobResultStepsTemplate<'a> {
    result: &'a JobResult,
}

#[derive(Template)]
#[template(path = "job-result-abort-button.html")]
pub struct JobResultAbortButtonTemplate<'a> {
    result: &'a JobResult,
}

fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;

    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

pub async fn template_job_results(query: Query<JobResultsQuery>) -> Response {
    match JobResult::get_all(query.job_id.clone()) {
        Ok(results) => {
            let has_in_progress = results.iter().any(|r| r.finished_at.is_none());
            let template = JobResultsTemplate {
                title: "Job Results".to_string(),
                has_in_progress,
                job_id_filter: query.job_id.as_deref(),
            };
            Html(template.render().unwrap()).into_response()
        }
        Err(e) => {
            eprintln!("Failed to get all job results: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn template_job_results_table(query: Query<JobResultsQuery>) -> Response {
    match JobResult::get_all(query.job_id.clone()) {
        Ok(results) => {
            let template = JobResultsTableTemplate { results };
            Html(template.render().unwrap()).into_response()
        }
        Err(e) => {
            eprintln!("Failed to get all job results for table: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn template_job_result(Path(id): Path<String>) -> Response {
    match JobResult::get(&id) {
        Ok(Some(result)) => {
            let template = JobResultTemplate {
                title: "Job Result",
                result: &result,
            };
            Html(template.render().unwrap()).into_response()
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

pub async fn template_job_result_logs(Path(result_id): Path<String>) -> Response {
    match JobResult::get(&result_id) {
        Ok(Some(result)) => {
            let logger = match result.logger.lock() {
                Ok(logger) => logger.clone(),
                Err(_) => {
                    eprintln!("Failed to lock logger for job result {}", result_id);
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

            match logger.get_logs() {
                Ok(logs) => {
                    let formatted_logs: Vec<FormattedLog> = logs
                        .iter()
                        .map(|log| FormattedLog {
                            timestamp: &log.timestamp,
                            level: &log.level,
                            message: &log.message,
                        })
                        .collect();

                    let template = JobResultLogsTemplate { logs: formatted_logs };
                    Html(template.render().unwrap()).into_response()
                }
                Err(e) => {
                    eprintln!("Failed to get logs for job result {}: {}", result_id, e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Ok(None) => {
            eprintln!("Job result not found: {}", result_id);
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            eprintln!("Failed to get job result {}: {}", result_id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn template_job_result_dynamic_content(Path((id, content_type)): Path<(String, String)>) -> Response {
    match JobResult::get(&id) {
        Ok(Some(result)) => {
            let now = Utc::now();
            let template = match content_type.as_str() {
                "header" => {
                    let template = JobResultHeaderTemplate {
                        result: &result,
                        running_for: format_duration((now - result.started_at).num_seconds()),
                    };
                    template.render().unwrap()
                }
                "steps" => {
                    let template = JobResultStepsTemplate { result: &result };
                    template.render().unwrap()
                }
                "abort-button" => {
                    let template = JobResultAbortButtonTemplate { result: &result };
                    template.render().unwrap()
                }
                _ => {
                    eprintln!("Invalid content type: {}", content_type);
                    return StatusCode::BAD_REQUEST.into_response();
                }
            };
            Html(template).into_response()
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
