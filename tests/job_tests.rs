use std::path::PathBuf;

use nomos_rust::job::{default_job_results_location, Job, JobExecutor, JobResult};
use nomos_rust::script::models::{Script, ScriptStatus, ScriptStep};
use nomos_rust::script::types::{BashScript, ScriptType};
use nomos_rust::script::ScriptParameterType;

#[test]
fn read_yml() {
    let path_buf = PathBuf::from("tests/jobs/test-job.yml");
    let job = Job::try_from(path_buf);
    assert!(job.is_ok());
    assert_eq!(
        job.unwrap().parameters[0].default,
        Some(ScriptParameterType::String("1".to_string()))
    );
}

#[tokio::test]
async fn execute_job() {
    do_execute_job().await;
}

async fn do_execute_job() -> String {
    let path_buf = PathBuf::from("tests/jobs/test-job.yml");
    let job = Job::try_from(path_buf).unwrap();
    let script = Script::try_from(PathBuf::from("tests/scripts/test-script.yml")).unwrap();
    let job_executor = JobExecutor::new();
    let result = job_executor
        .execute_with_script(&job, Default::default(), &script)
        .await
        .unwrap();
    let result = JobResult::wait_for_completion(&result).await.unwrap();

    assert!(result.finished_at.is_some());
    assert_eq!(result.status, ScriptStatus::Success);
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.current_step_name.unwrap(), "Test Step");
    assert!(result.finished_at.unwrap() > result.started_at);
    for step in result.steps {
        assert_eq!(step.status, ScriptStatus::Success);
        assert!(step.finished_at > step.started_at);
    }

    result.id
}

#[tokio::test]
async fn job_result_id_test() {
    let thread_1 = tokio::spawn(async { do_execute_job().await });
    let thread_2 = tokio::spawn(async { do_execute_job().await });
    let thread_3 = tokio::spawn(async { do_execute_job().await });
    let thread_4 = tokio::spawn(async { do_execute_job().await });

    let id_1 = thread_1.await.unwrap();
    let id_2 = thread_2.await.unwrap();
    let id_3 = thread_3.await.unwrap();
    let id_4 = thread_4.await.unwrap();

    assert_ne!(id_1, id_2);
    assert_ne!(id_1, id_3);
    assert_ne!(id_1, id_4);
    assert_ne!(id_2, id_3);
    assert_ne!(id_2, id_4);
    assert_ne!(id_3, id_4);
}

#[tokio::test]
async fn git_job() {
    let path_buf = PathBuf::from("tests/jobs/git-job.yml");
    let job = Job::try_from(path_buf).unwrap();
    let script = Script::try_from(PathBuf::from("tests/scripts/git-script.yml")).unwrap();
    let job_executor = JobExecutor::new();
    let result = job_executor
        .execute_with_script(&job, Default::default(), &script)
        .await
        .unwrap();
    let result = JobResult::wait_for_completion(&result).await.unwrap();
    let result_directory = default_job_results_location().unwrap().join(&result.id);
    assert!(result.finished_at.is_some());
    assert_eq!(result.status, ScriptStatus::Success);
    assert_eq!(result.steps.len(), 3);
    for step in result.steps {
        assert_eq!(step.status, ScriptStatus::Success);
        assert!(step.finished_at.unwrap() > step.started_at.unwrap());
    }
    assert!(result_directory.join("result.yml").exists());
    assert!(result_directory.join("log").exists());
    assert!(!result_directory.join("Hello-World").exists());
}

#[tokio::test]
async fn docker_job() {
    let path_buf = PathBuf::from("tests/jobs/docker-job.yml");
    let job = Job::try_from(path_buf).unwrap();
    let script = Script::try_from(PathBuf::from("tests/scripts/docker-script.yml")).unwrap();
    let job_executor = JobExecutor::new();
    let result = job_executor
        .execute_with_script(&job, Default::default(), &script)
        .await
        .unwrap();
    let result = JobResult::wait_for_completion(&result).await.unwrap();
    assert!(result.finished_at.is_some());
    assert_eq!(result.status, ScriptStatus::Success);
    assert_eq!(result.steps.len(), 4);
    for step in result.steps {
        assert_eq!(step.status, ScriptStatus::Success);
        assert!(step.finished_at.unwrap() > step.started_at.unwrap());
    }
}

#[tokio::test]
async fn validation() {
    // Missing git step
    let script = Script {
        steps: vec![ScriptStep {
            name: "Test Step".to_string(),
            values: vec![ScriptType::Bash(BashScript {
                code: "echo $(missing.param)".to_string(),
            })],
        }],
        id: "test-script".to_string(),
        name: "Test Script".to_string(),
        parameters: vec![],
    };
    let job = Job {
        id: "test-job".to_string(),
        name: "Test Job".to_string(),
        parameters: vec![],
        triggers: vec![],
        script_id: "test-script".to_string(),
        read_only: false,
    };
    let result = job.validate(Some(&script), Default::default()).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Error in step Test Step: Parameter 'missing.param' not found"
    );
}
