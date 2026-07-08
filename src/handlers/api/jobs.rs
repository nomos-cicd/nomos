use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use crate::{
    credential::{Credential, CredentialType},
    job::{GithubPayload, Job, TriggerType},
    script::ScriptParameterType,
    utils::is_signature_valid,
    AppState,
};

#[derive(Deserialize)]
pub struct JobsQuery {
    #[serde(rename = "script-id")]
    script_id: Option<String>,
}

pub async fn get_jobs(Query(query): Query<JobsQuery>) -> Response {
    let jobs = Job::get_all().unwrap_or_default();
    let filtered_jobs: Vec<Job> = jobs
        .into_iter()
        .filter(|job| query.script_id.as_ref().is_none_or(|id| job.script_id == *id))
        .collect();

    Json(filtered_jobs).into_response()
}

pub async fn get_job(Path(id): Path<String>) -> Response {
    match Job::get(&id) {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Failed to get job {}: {}", id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn create_job(headers: HeaderMap, body: String) -> Response {
    let content_type = match headers.get("content-type") {
        Some(ct) => ct.to_str().unwrap_or(""),
        None => return (StatusCode::BAD_REQUEST, "Empty content-type").into_response(),
    };

    if content_type != "application/yaml" {
        return (StatusCode::BAD_REQUEST, "Only application/yaml is supported").into_response();
    }

    match serde_yaml::from_str::<Job>(&body) {
        Ok(job) => match job.sync(None).await {
            Ok(_) => (StatusCode::CREATED, job.id).into_response(),
            Err(e) => {
                eprintln!("Failed to sync job: {}", e);
                (StatusCode::BAD_REQUEST, e).into_response()
            }
        },
        Err(e) => {
            eprintln!("Failed to parse job YAML: {}", e);
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

pub async fn execute_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(parameters): Json<HashMap<String, ScriptParameterType>>,
) -> Response {
    match Job::get(&id) {
        Ok(Some(job)) => match job.execute(&state.job_executor, parameters).await {
            Ok(job_result_id) => job_result_id.into_response(),
            Err(e) => {
                eprintln!("Failed to execute job {}: {}", id, e);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Failed to get job {}: {}", id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn delete_job(Path(id): Path<String>) -> Response {
    match Job::get(&id) {
        Ok(Some(job)) => match job.delete() {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => {
                eprintln!("Failed to delete job {}: {}", id, e);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("Failed to get job {}: {}", id, e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn dry_run_job(headers: HeaderMap, body: String) -> Response {
    let content_type = match headers.get("content-type") {
        Some(ct) => ct.to_str().unwrap_or(""),
        None => return (StatusCode::BAD_REQUEST, "Empty content-type").into_response(),
    };

    if content_type != "application/yaml" {
        return (StatusCode::BAD_REQUEST, "Only application/yaml is supported").into_response();
    }

    match serde_yaml::from_str::<Job>(&body) {
        Ok(job) => match job.validate(None, Default::default()).await {
            Ok(_) => StatusCode::OK.into_response(),
            Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
        },
        Err(e) => {
            eprintln!("Failed to parse job YAML: {}", e);
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

pub async fn job_webhook_trigger(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    match Job::get_all() {
        Ok(jobs) => {
            let mut github_payload: Option<GithubPayload> = None;

            for job in jobs {
                for trigger in job.triggers.iter() {
                    match trigger {
                        TriggerType::Github(val) => {
                            let signature = match headers.get("x-hub-signature-256") {
                                Some(signature) => match signature.to_str() {
                                    Ok(signature) => signature,
                                    Err(e) => {
                                        eprintln!("Invalid GitHub signature header: {}", e);
                                        continue;
                                    }
                                },
                                None => {
                                    eprintln!("Signature not found in headers");
                                    continue;
                                }
                            };

                            let github_event = match headers.get("x-github-event") {
                                Some(github_event) => match github_event.to_str() {
                                    Ok(github_event) => github_event,
                                    Err(e) => {
                                        eprintln!("Invalid GitHub event header: {}", e);
                                        continue;
                                    }
                                },
                                None => {
                                    eprintln!("Event not found in headers");
                                    continue;
                                }
                            };

                            if github_payload.is_none() {
                                match serde_json::from_str::<GithubPayload>(&body) {
                                    Ok(payload) => github_payload = Some(payload),
                                    Err(e) => {
                                        eprintln!("Failed to parse GitHub payload: {}", e);
                                        continue;
                                    }
                                }
                            }
                            let Some(payload) = github_payload.as_ref() else {
                                continue;
                            };

                            match Credential::get(val.secret_credential_id.as_str(), None) {
                                Ok(Some(credential)) => {
                                    let text_credential = match credential.value {
                                        CredentialType::Text(val) => Some(val),
                                        _ => {
                                            eprintln!("Credential is not Text: {}", val.secret_credential_id);
                                            None
                                        }
                                    };

                                    if let Some(text_credential) = text_credential {
                                        match is_signature_valid(&body, signature, &text_credential.value) {
                                            Ok(is_valid) => {
                                                if !is_valid {
                                                    eprintln!("Invalid signature");
                                                    continue;
                                                }

                                                if payload.repository.full_name != val.url {
                                                    eprintln!("Repository does not match");
                                                    continue;
                                                }

                                                if !val.events.iter().any(|x| x == github_event) {
                                                    eprintln!("Event does not match");
                                                    continue;
                                                }

                                                let mut params = HashMap::new();
                                                params.insert(
                                                    "github_payload".to_string(),
                                                    ScriptParameterType::String(body.clone()),
                                                );

                                                match job.execute(&state.job_executor, params).await {
                                                    Ok(result) => eprintln!("Job started: {}", result),
                                                    Err(e) => eprintln!("Failed to execute job: {}", e),
                                                }
                                            }
                                            Err(e) => eprintln!("Failed to validate signature: {}", e),
                                        }
                                    }
                                }
                                Ok(None) => eprintln!("Credential not found: {}", val.secret_credential_id),
                                Err(e) => eprintln!("Failed to get credential: {}", e),
                            }
                        }
                        TriggerType::Manual(_) => {}
                    }
                }
            }
            StatusCode::OK.into_response()
        }
        Err(e) => {
            eprintln!("Failed to get jobs for webhook trigger: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
