use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use sysinfo::{Pid, System};
use tokio::{
    sync::Mutex,
    task::{self},
};

use crate::{
    job::models::{Job, JobResult},
    script::{
        models::{Script, ScriptStatus},
        ScriptExecutionContext, ScriptExecutor, ScriptParameterType,
    },
    utils::get_process_recursive,
};
#[derive(Debug, Clone)]
pub struct JobExecutor {
    handles: Arc<Mutex<HashMap<String, task::AbortHandle>>>,
}

impl Default for JobExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl JobExecutor {
    pub fn new() -> Self {
        JobExecutor {
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn execute_with_script(
        &self,
        job: &Job,
        parameters: HashMap<String, ScriptParameterType>,
        script: &Script,
    ) -> Result<String, String> {
        job.validate_parameters(Some(script))?;

        let mut merged_parameters = job.merged_parameters(Some(script), parameters.clone())?;
        let job_result = JobResult::try_from((job, script, false))?;
        let id = job_result.id.clone();
        let cloned_id = id.clone();
        let other_id = id.clone();
        let handles = Arc::clone(&self.handles);

        let directory = crate::job::utils::default_job_results_location()?.join(&job_result.id);
        fs::create_dir_all(&directory).map_err(|e| format!("Failed to create job result directory: {}", e))?;
        let cleanup_directory = directory.clone();

        job_result.save()?;

        let mut job_result_clone = job_result.clone();
        let handle = task::spawn(async move {
            let _res =
                Self::execute_job_result_internal(&mut job_result_clone, &directory, &mut merged_parameters).await;
        });
        let abort_handle = handle.abort_handle();
        self.handles.lock().await.insert(id, abort_handle);

        task::spawn(async move {
            match handle.await {
                Ok(_) => {}
                Err(e) => {
                    if e.is_cancelled() {
                        let message = format!("Cancelled job {}: {}", other_id, e);
                        match JobResult::get(other_id.as_str()) {
                            Ok(Some(mut job_result)) => {
                                job_result.add_log(crate::log::LogLevel::Error, message.clone());
                                let s = System::new_all();
                                for child_process in &job_result.child_process_ids {
                                    let mut processes = get_process_recursive(*child_process);
                                    processes.reverse(); // Kill child processes first
                                    processes.push(Pid::from(*child_process));
                                    eprintln!("Killing processes with PID {}", child_process);
                                    for process in processes {
                                        if let Some(process) = s.process(process) {
                                            job_result.add_log(
                                                crate::log::LogLevel::Info,
                                                format!("Killing process with PID {}", process.pid()),
                                            );
                                            process.kill();
                                        } else {
                                            eprintln!("Process with PID {} not found", process);
                                        }
                                    }
                                }
                                match job_result.finish_step(ScriptStatus::Aborted) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("Failed to finish step: {}", e);
                                    }
                                }
                                job_result.child_process_ids.clear();
                                job_result.status = ScriptStatus::Aborted;
                                match job_result.save() {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("Failed to save job result: {}", e);
                                    }
                                }
                                cleanup_job_workspace(&cleanup_directory, &job_result);
                            }
                            Ok(None) => {
                                eprintln!("{}", message);
                            }
                            Err(e) => {
                                eprintln!("Failed to get job result: {}", e);
                            }
                        }
                    }
                }
            }
            handles.lock().await.remove(&other_id);
        });

        Ok(cloned_id)
    }

    async fn execute_job_result_internal(
        job_result: &mut JobResult,
        directory: &Path,
        parameters: &mut HashMap<String, ScriptParameterType>,
    ) -> Result<(), String> {
        let mut is_success = true;

        while job_result.finished_at.is_none() {
            job_result.start_step()?;

            let current_step = job_result
                .get_current_step_mut()
                .ok_or("No current step found")?
                .clone();

            let step_name = current_step.name.clone();

            let mut context = ScriptExecutionContext {
                parameters,
                directory,
                step_name: &step_name,
                job_result,
            };

            if let Err(e) = current_step.execute(&mut context).await {
                let message = format!("Error in step {}: {}", step_name, e);
                job_result.add_log(crate::log::LogLevel::Error, message.clone());
                job_result.finish_step(ScriptStatus::Failed)?;
                is_success = false;

                if job_result.dry_run {
                    return Err(message);
                }
                break;
            }

            if let Err(e) = job_result.finish_step(ScriptStatus::Success) {
                let message = format!("Error finishing step {}: {}", step_name, e);
                job_result.add_log(crate::log::LogLevel::Error, message.clone());
                is_success = false;

                if job_result.dry_run {
                    return Err(message);
                }
                break;
            }
        }

        job_result.status = if is_success {
            ScriptStatus::Success
        } else {
            ScriptStatus::Failed
        };
        job_result.save()?;
        cleanup_job_workspace(directory, job_result);

        Ok(())
    }

    pub async fn validate(
        &self,
        job: &Job,
        script: &Script,
        parameters: HashMap<String, ScriptParameterType>,
    ) -> Result<(), String> {
        let mut merged_parameters = job.merged_parameters(Some(script), parameters)?;
        let mut job_result = JobResult::try_from((job, script, true))?;
        let directory = PathBuf::from("tmp");

        Self::execute_job_result_internal(&mut job_result, &directory, &mut merged_parameters).await
    }

    pub async fn stop_job(&self, id: &str) -> Result<(), String> {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.get(id) {
            handle.abort();
            handles.remove(id);
            Ok(())
        } else {
            Err(format!("Job {} not found", id))
        }
    }
}

fn cleanup_job_workspace(directory: &Path, job_result: &JobResult) {
    if job_result.dry_run {
        return;
    }

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) => {
            job_result.add_log(
                crate::log::LogLevel::Warning,
                format!("Failed to read job workspace for cleanup: {}", e),
            );
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                job_result.add_log(
                    crate::log::LogLevel::Warning,
                    format!("Failed to read job workspace entry for cleanup: {}", e),
                );
                continue;
            }
        };
        let path = entry.path();
        let file_name = entry.file_name();
        if file_name == "result.yml" || file_name == "log" {
            continue;
        }

        let result = match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => fs::remove_dir_all(&path),
            Ok(_) => fs::remove_file(&path),
            Err(e) => {
                job_result.add_log(
                    crate::log::LogLevel::Warning,
                    format!("Failed to inspect job workspace entry {}: {}", path.display(), e),
                );
                continue;
            }
        };

        if let Err(e) = result {
            job_result.add_log(
                crate::log::LogLevel::Warning,
                format!("Failed to remove job workspace entry {}: {}", path.display(), e),
            );
        }
    }
}
