use std::process::Stdio;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use sysinfo::{Pid, System};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
};

use crate::script::ScriptExecutionContext;

use crate::log::LogLevel;

pub async fn execute_command(command: &str, context: &mut ScriptExecutionContext<'_>) -> Result<(), String> {
    let child = if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd.current_dir(context.directory);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd.current_dir(context.directory);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?
    };

    execute_script(child, context).await
}

pub async fn execute_command_with_env(
    command: &str,
    env: Vec<(String, String)>,
    context: &mut ScriptExecutionContext<'_>,
) -> Result<(), String> {
    let child = if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]).current_dir(context.directory);
        cmd.kill_on_drop(true);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command).current_dir(context.directory);
        cmd.kill_on_drop(true);
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?
    };

    execute_script(child, context).await
}

async fn execute_script(mut child: Child, context: &mut ScriptExecutionContext<'_>) -> Result<(), String> {
    let raw_child_pid = child.id().ok_or_else(|| "Failed to get child process id".to_string())?;
    eprintln!("Child process id: {}", raw_child_pid);
    let child_pid = raw_child_pid
        .try_into()
        .map_err(|_| "Child process id is too large".to_string())?;
    context.job_result.child_process_ids.push(child_pid);
    context.job_result.save()?;

    let stdout = child.stdout.take().ok_or_else(|| {
        context.job_result.child_process_ids.retain(|pid| *pid != child_pid);
        "Failed to open stdout".to_string()
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        context.job_result.child_process_ids.retain(|pid| *pid != child_pid);
        "Failed to open stderr".to_string()
    })?;

    let stdout_handle = read_child_output(stdout, context.job_result.clone(), LogLevel::Info);
    let stderr_handle = read_child_output(stderr, context.job_result.clone(), LogLevel::Error);

    let status = child.wait().await.map_err(|e| e.to_string());
    let stdout_result = stdout_handle.await.map_err(|e| e.to_string());
    let stderr_result = stderr_handle.await.map_err(|e| e.to_string());
    context.job_result.child_process_ids.retain(|pid| *pid != child_pid);

    let status = status?;
    stdout_result?;
    stderr_result?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Process exited with status: {}", status))
    }
}

fn read_child_output<R>(output: R, job_result: crate::job::JobResult, level: LogLevel) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(output).lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.is_empty() {
                        job_result.add_log(level.clone(), line);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("Failed to read child process output: {}", e);
                    break;
                }
            }
        }
    })
}

type HmacSha256 = Hmac<Sha256>;
pub fn is_signature_valid(payload: &str, signature: &str, secret: &str) -> Result<bool, String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let result = format!("sha256={}", hex::encode(result.into_bytes()));
    Ok(result == signature)
}

pub fn get_process_recursive(pid: usize) -> Vec<Pid> {
    let s = System::new_all();
    let root_pid = Pid::from(pid);
    let processes = s.processes();

    let mut result = Vec::new();
    let mut last_list = [root_pid].to_vec();
    while !last_list.is_empty() {
        let mut new_list = Vec::new();
        for parent_pid in last_list {
            for (child_pid, process) in processes {
                if process.parent() == Some(parent_pid) {
                    new_list.push(*child_pid);
                    result.push(*child_pid);
                }
            }
        }
        last_list = new_list;
    }

    result
}
