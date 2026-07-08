use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::{job::JobResult, AppState};

#[derive(Deserialize)]
pub struct JobResultsQuery {
    #[serde(rename = "job-id")]
    job_id: Option<String>,
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

pub async fn get_job_result_logs(Path(id): Path<String>) -> Response {
    match JobResult::get(&id) {
        Ok(Some(result)) => {
            if let Ok(logger) = result.logger.lock() {
                match logger.get_logs() {
                    Ok(logs) => {
                        let text = logs
                            .iter()
                            .map(|log| {
                                format!(
                                    "[{}] [{}] {}",
                                    log.timestamp.format("%Y-%m-%d %H:%M:%S"),
                                    log.level,
                                    log.message
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        match Response::builder()
                            .header(header::CONTENT_TYPE, "text/plain")
                            .body(text)
                        {
                            Ok(response) => response.into_response(),
                            Err(e) => {
                                eprintln!("Failed to build log response for job result {}: {}", id, e);
                                StatusCode::INTERNAL_SERVER_ERROR.into_response()
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to get logs for job result {}: {}", id, e);
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                }
            } else {
                eprintln!("Failed to lock logger for job result {}", id);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
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
