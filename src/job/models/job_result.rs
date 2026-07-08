use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    job::{models::Job, utils::default_job_results_location},
    log::{JobLogger, LogLevel},
    script::models::{RunningScriptStep, Script, ScriptStatus},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct JobResult {
    pub id: String,
    pub job_id: String,
    pub status: ScriptStatus,
    pub steps: Vec<RunningScriptStep>,
    pub current_step_name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub logger: Arc<Mutex<JobLogger>>,
    #[serde(skip)]
    pub dry_run: bool,
    pub child_process_ids: Vec<usize>,
}

impl JobResult {
    pub fn new(
        id: String,
        job_id: String,
        steps: Vec<RunningScriptStep>,
        logger: Arc<Mutex<JobLogger>>,
        dry_run: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            job_id,
            steps: steps.clone(),
            current_step_name: steps.first().map(|step| step.name.clone()),
            status: ScriptStatus::Failed,
            started_at: now,
            updated_at: now,
            finished_at: None,
            logger,
            dry_run,
            child_process_ids: vec![],
        }
    }

    pub fn get_current_step_mut(&mut self) -> Option<&mut RunningScriptStep> {
        self.current_step_name
            .as_ref()
            .and_then(|name| self.steps.iter_mut().find(|step| step.name == *name))
    }

    pub fn start_step(&mut self) -> Result<(), String> {
        match self.get_current_step_mut() {
            Some(step) => {
                step.start();
                self.save()
            }
            None => Err("No current step".to_string()),
        }
    }

    pub fn finish_step(&mut self, status: ScriptStatus) -> Result<(), String> {
        let now = Utc::now();

        let current_step_name = self.current_step_name.clone().ok_or("No current step")?;

        if let Some(current_step) = self.get_current_step_mut() {
            current_step.finish(status.clone());
        } else {
            return Err("Failed to get current step".to_string());
        }

        if status != ScriptStatus::Success {
            self.status = status;
            self.updated_at = now;
            self.finished_at = Some(now);
            self.save()?;
            return Ok(());
        }

        if let Some(index) = self.steps.iter().position(|step| step.name == current_step_name) {
            if index + 1 < self.steps.len() {
                self.current_step_name = Some(self.steps[index + 1].name.clone());
                self.updated_at = now;
            } else {
                self.updated_at = now;
                self.finished_at = Some(now);
            }
            self.save()?;
        }

        Ok(())
    }

    pub fn add_log(&self, level: LogLevel, message: String) {
        eprintln!("{:?}: {}", level, message);

        if self.dry_run {
            return;
        }

        if let Ok(mut logger) = self.logger.lock() {
            let step_name = self.current_step_name.as_deref().unwrap_or("");
            if let Err(e) = logger.log(level, step_name, &message) {
                eprintln!("Failed to log message: {}", e);
            }
        }
    }

    pub fn get_all(job_id: Option<String>) -> Result<Vec<Self>, String> {
        let path = default_job_results_location()?;
        let mut job_results = Vec::new();

        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let mut path = entry.path();
            path.push("result.yml");

            match JobResult::try_from(path.clone()) {
                Ok(result) => {
                    if let Some(ref job_id) = job_id {
                        if result.job_id == *job_id {
                            job_results.push(result);
                        }
                    } else {
                        job_results.push(result);
                    }
                }
                Err(e) => eprintln!("Error reading job result: Path: {:?}, Error: {}", path, e),
            }
        }

        job_results.sort_by_key(|result| Reverse(result.started_at));
        Ok(job_results)
    }

    pub fn get(id: &str) -> Result<Option<Self>, String> {
        let path = default_job_results_location()?.join(id).join("result.yml");
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_yaml::from_str(&content).map_err(|e| e.to_string())
    }

    pub fn save(&self) -> Result<(), String> {
        if self.dry_run {
            return Ok(());
        }

        let path = default_job_results_location()?.join(&self.id).join("result.yml");
        let file = File::create(path).map_err(|e| e.to_string())?;
        serde_yaml::to_writer(file, self).map_err(|e| e.to_string())
    }

    #[allow(dead_code)]
    pub async fn wait_for_completion(id: &str) -> Result<Self, String> {
        let mut job_result = Self::get(id)?.ok_or("Job result not found")?;

        while job_result.finished_at.is_none() {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            job_result = Self::get(id)?.ok_or("Job result not found")?;
        }

        Ok(job_result)
    }

    pub fn create_dummy() -> Self {
        Self::new(
            "dummy".to_string(),
            "dummy".to_string(),
            vec![],
            Arc::new(Mutex::new(
                JobLogger::new("dummy".to_string(), "dummy".to_string(), true).unwrap(),
            )),
            false,
        )
    }
}

impl TryFrom<PathBuf> for JobResult {
    type Error = String;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let file = File::open(&path).map_err(|e| format!("Could not open file: {}", e))?;
        let reader = BufReader::new(file);
        serde_yaml::from_reader(reader).map_err(|e| e.to_string())
    }
}

impl TryFrom<&Job> for JobResult {
    type Error = String;

    fn try_from(job: &Job) -> Result<Self, Self::Error> {
        let id = crate::job::utils::next_job_result_id()?;
        let script =
            Script::get(&job.script_id)?.ok_or_else(|| format!("Script with id '{}' not found", job.script_id))?;

        let steps: Vec<RunningScriptStep> = script.steps.iter().map(RunningScriptStep::from).collect();
        let logger = Arc::new(Mutex::new(JobLogger::new(job.id.clone(), id.clone(), false)?));

        Ok(Self::new(id, job.id.clone(), steps, logger, false))
    }
}

impl TryFrom<(&Job, &Script, bool)> for JobResult {
    type Error = String;

    fn try_from((job, script, dry_mode): (&Job, &Script, bool)) -> Result<Self, Self::Error> {
        let id = if !dry_mode {
            crate::job::utils::next_job_result_id()?
        } else {
            "dry_run".to_string()
        };

        let steps: Vec<RunningScriptStep> = script.steps.iter().map(RunningScriptStep::from).collect();
        let logger = Arc::new(Mutex::new(JobLogger::new(job.id.clone(), id.clone(), dry_mode)?));

        Ok(Self::new(id, job.id.clone(), steps, logger, dry_mode))
    }
}

impl Clone for JobResult {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            job_id: self.job_id.clone(),
            status: self.status.clone(),
            steps: self.steps.clone(),
            current_step_name: self.current_step_name.clone(),
            started_at: self.started_at,
            updated_at: self.updated_at,
            finished_at: self.finished_at,
            logger: Arc::clone(&self.logger),
            dry_run: self.dry_run,
            child_process_ids: self.child_process_ids.clone(),
        }
    }
}
