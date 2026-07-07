use tempfile::NamedTempFile;

use crate::script::ScriptExecutionContext;

use crate::{
    credential::{Credential, CredentialType},
    log::LogLevel,
    utils::{execute_command, execute_command_with_env},
};

pub async fn git_clone(
    url: &str,
    branch: &str,
    credential_id: Option<&str>,
    context: &mut ScriptExecutionContext<'_>,
) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        if !context.job_result.dry_run {
            // Workaround for local
            execute_command(&format!("git clone -b {} {}", branch, url), context).await?;
        }

        Ok(())
    } else if let Some(cred_id) = credential_id {
        let credential = match Credential::get(cred_id, Some(context.job_result))? {
            Some(cred) => cred,
            None => return Err(format!("Credential not found: {}", cred_id)),
        };

        match credential.value {
            CredentialType::Ssh(ssh_credential) => {
                context
                    .job_result
                    .add_log(LogLevel::Info, "command: chmod 400 <private_key_temp_file>".to_string());
                context
                    .job_result
                    .add_log(LogLevel::Info, format!("command: git clone -b {} {}", branch, url));
                if !context.job_result.dry_run {
                    let tmp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
                    let tmp_path = tmp_file.path();
                    std::fs::write(tmp_path, ssh_credential.private_key).map_err(|e| e.to_string())?;

                    execute_command(&format!("chmod 400 {}", tmp_path.display()), context).await?;

                    let env = vec![(
                        "GIT_SSH_COMMAND".to_string(),
                        format!("ssh -i {} -o StrictHostKeyChecking=no", tmp_path.display()),
                    )];
                    execute_command_with_env(&format!("git clone -b {} {}", branch, url), env, context).await?;
                }

                Ok(())
            }
            _ => Err("Invalid credential type".into()),
        }
    } else {
        Err("Credential ID is required".into())
    }
}

pub async fn git_pull(
    directory: &str,
    lfs: bool,
    credential_id: Option<&str>,
    context: &mut ScriptExecutionContext<'_>,
) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        if !context.job_result.dry_run {
            let mut command = format!("cd {} && ", directory);
            if lfs {
                command.push_str("git lfs pull");
            } else {
                command.push_str("git pull");
            }
            execute_command(&command, context).await?;
        }
        Ok(())
    } else if let Some(cred_id) = credential_id {
        let credential = match Credential::get(cred_id, Some(context.job_result))? {
            Some(cred) => cred,
            None => return Err(format!("Credential not found: {}", cred_id)),
        };

        match credential.value {
            CredentialType::Ssh(ssh_credential) => {
                let log_command = if lfs {
                    "git lfs pull".to_string()
                } else {
                    "git pull".to_string()
                };
                context
                    .job_result
                    .add_log(LogLevel::Info, format!("command: {}", log_command));
                if !context.job_result.dry_run {
                    let tmp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
                    let tmp_path = tmp_file.path();
                    std::fs::write(tmp_path, ssh_credential.private_key).map_err(|e| e.to_string())?;
                    execute_command(&format!("chmod 400 {}", tmp_path.display()), context).await?;
                    let env = vec![(
                        "GIT_SSH_COMMAND".to_string(),
                        format!("ssh -i {} -o StrictHostKeyChecking=no", tmp_path.display()),
                    )];
                    let mut command = format!("cd {} && ", directory);
                    if lfs {
                        command.push_str("git lfs pull");
                    } else {
                        command.push_str("git pull");
                    }
                    execute_command_with_env(&command, env, context).await?;
                }
                Ok(())
            }
            _ => Err("Invalid credential type".into()),
        }
    } else {
        Err("Credential ID is required".into())
    }
}
