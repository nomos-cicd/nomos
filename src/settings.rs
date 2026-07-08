use std::{collections::HashSet, path::PathBuf};

use serde::Deserialize;

use crate::{
    credential::Credential,
    job::{Job, JobResult},
    log::LogLevel,
    script::models::Script,
};

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub credentials: Vec<Credential>,
}

impl Settings {
    pub async fn sync(&self, job_result: &mut JobResult) -> Result<(), String> {
        let mut credential_ids = HashSet::new();
        for credential in &self.credentials {
            tokio::task::yield_now().await;
            if credential.read_only {
                job_result.add_log(
                    LogLevel::Info,
                    format!("Skipping read-only credential {:?}", credential.id),
                );
                continue;
            }

            let res = credential.sync(&mut job_result.into());
            if let Err(e) = res {
                job_result.add_log(LogLevel::Error, format!("Error syncing credential: {:?}", e));
                continue;
            }
            credential_ids.insert(credential.id.clone());
        }

        let credentials = Credential::get_all()?;
        for credential in credentials {
            tokio::task::yield_now().await;
            if !credential_ids.contains(&credential.id) && !credential.read_only {
                let res = credential.delete();
                if let Err(e) = res {
                    job_result.add_log(LogLevel::Error, format!("Error deleting credential: {:?}", e));
                    continue;
                }
            }
        }

        Ok(())
    }
}

impl TryFrom<PathBuf> for Settings {
    type Error = String;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let yaml_str = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let settings: Settings = serde_yaml::from_str(yaml_str.as_str()).map_err(|e| e.to_string())?;
        Ok(settings)
    }
}

pub async fn sync(directory: PathBuf, job_result: &mut JobResult) -> Result<(), String> {
    if job_result.dry_run {
        job_result.add_log(LogLevel::Info, "Dry run enabled, skipping sync".to_string());
        return Ok(());
    }

    tokio::task::yield_now().await;
    let settings_path = directory.join("settings.yml");
    if settings_path.exists() {
        let settings = Settings::try_from(settings_path)?;
        settings.sync(job_result).await?;
    } else {
        job_result.add_log(LogLevel::Info, "No settings file found".to_string());
    }

    tokio::task::yield_now().await;
    let scripts_path = directory.join("scripts");
    if scripts_path.exists() {
        let mut script_ids = HashSet::new();
        for entry in std::fs::read_dir(scripts_path).map_err(|e| e.to_string())? {
            tokio::task::yield_now().await;
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            match Script::try_from(path) {
                Ok(script) => match script.sync(job_result.into()) {
                    Ok(_) => {
                        script_ids.insert(script.id.clone());
                    }
                    Err(e) => job_result.add_log(LogLevel::Error, format!("Error syncing script: {:?}", e)),
                },
                Err(e) => job_result.add_log(LogLevel::Error, format!("Error creating script: {:?}", e)),
            }
        }
        let scripts = Script::get_all()?;
        for script in scripts {
            tokio::task::yield_now().await;
            if !script_ids.contains(&script.id) {
                match script.delete() {
                    Ok(_) => job_result.add_log(LogLevel::Info, format!("Deleted script {:?}", script.id)),
                    Err(e) => job_result.add_log(LogLevel::Error, format!("Error deleting script: {:?}", e)),
                }
            }
        }
    } else {
        job_result.add_log(LogLevel::Info, "No scripts directory found".to_string());
    }

    tokio::task::yield_now().await;
    let jobs_path = directory.join("jobs");
    if jobs_path.exists() {
        tokio::task::yield_now().await;
        let mut job_ids = HashSet::new();
        for entry in std::fs::read_dir(jobs_path).map_err(|e| e.to_string())? {
            tokio::task::yield_now().await;
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            match Job::try_from(path) {
                Ok(job) => {
                    if job.read_only {
                        job_result.add_log(LogLevel::Info, format!("Skipping read-only job {:?}", job.id));
                        continue;
                    }
                    match job.sync(job_result.into()).await {
                        Ok(_) => {
                            job_ids.insert(job.id.clone());
                        }
                        Err(e) => job_result.add_log(LogLevel::Error, format!("Error syncing job: {:?}", e)),
                    }
                }
                Err(e) => job_result.add_log(LogLevel::Error, format!("Error creating job: {:?}", e)),
            }
        }
        tokio::task::yield_now().await;
        let jobs = Job::get_all()?;
        for job in jobs {
            tokio::task::yield_now().await;
            if !job_ids.contains(&job.id) && !job.read_only {
                match job.delete() {
                    Ok(_) => job_result.add_log(LogLevel::Info, format!("Deleted job {:?}", job.id)),
                    Err(e) => job_result.add_log(LogLevel::Error, format!("Error deleting job: {:?}", e)),
                }
            }
        }
    } else {
        job_result.add_log(LogLevel::Info, "No jobs directory found".to_string());
    }

    Ok(())
}
