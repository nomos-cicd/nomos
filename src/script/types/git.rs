use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    git::{git_clone, git_pull},
    script::{
        utils::{ParameterSubstitution, SubstitutionResult},
        ScriptExecutionContext, ScriptExecutor, ScriptParameterType,
    },
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GitCloneScript {
    pub url: String,
    pub credential_id: Option<String>,
    pub branch: Option<String>,
}

#[async_trait]
impl ScriptExecutor for GitCloneScript {
    async fn execute(&self, context: &mut ScriptExecutionContext<'_>) -> Result<(), String> {
        // Substitute parameters
        let url = self
            .url
            .substitute_parameters(context.parameters, false)?
            .ok_or("URL is required")?;
        let url = match url {
            SubstitutionResult::Single(s) => s,
            SubstitutionResult::Multiple(_) => {
                return Err("URL parameter cannot be an array".to_string());
            }
        };

        let credential_id = match &self.credential_id {
            Some(id) => id.substitute_parameters(context.parameters, true)?,
            None => None,
        };
        let credential_id = match credential_id {
            Some(id) => match id {
                SubstitutionResult::Single(s) => Some(s),
                SubstitutionResult::Multiple(_) => {
                    return Err("Credential ID parameter cannot be an array".to_string());
                }
            },
            None => None,
        };

        let branch: String = match &self.branch {
            Some(b) => {
                let branch = b.substitute_parameters(context.parameters, false)?;
                match branch {
                    Some(b) => match b {
                        SubstitutionResult::Single(s) => s,
                        SubstitutionResult::Multiple(_) => {
                            return Err("Branch parameter cannot be an array".to_string());
                        }
                    },
                    None => "main".to_string(),
                }
            }
            None => "main".to_string(),
        };

        tokio::task::yield_now().await;
        git_clone(&url, branch.as_str(), credential_id.as_deref(), context).await?;
        tokio::task::yield_now().await;

        let mut new_dir = match url.split('/').next_back() {
            Some(last_part) => context.directory.join(last_part),
            None => return Err("Invalid URL format".to_string()),
        };

        if let Some(dir_str) = new_dir.to_str() {
            if dir_str.ends_with(".git") {
                new_dir = match dir_str.strip_suffix(".git") {
                    Some(stripped) => PathBuf::from(stripped),
                    None => return Err("Failed to strip .git suffix".to_string()),
                };
            }
        } else {
            return Err("Invalid directory path".to_string());
        }

        let new_dir_str = match new_dir.to_str() {
            Some(s) => s,
            None => return Err("Invalid directory path".to_string()),
        };

        context.parameters.insert(
            format!("steps.{}.git-clone.directory", context.step_name),
            ScriptParameterType::String(new_dir_str.to_string()),
        );

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct GitPullScript {
    pub directory: String,
    pub credential_id: Option<String>,
    pub lfs: Option<String>,
}

#[async_trait]
impl ScriptExecutor for GitPullScript {
    async fn execute(&self, context: &mut ScriptExecutionContext<'_>) -> Result<(), String> {
        // Substitute parameters
        let directory = self
            .directory
            .substitute_parameters(context.parameters, false)?
            .ok_or("Directory is required")?;
        let directory = match directory {
            SubstitutionResult::Single(s) => s,
            SubstitutionResult::Multiple(_) => {
                return Err("Directory parameter cannot be an array".to_string());
            }
        };

        let credential_id = match &self.credential_id {
            Some(id) => id.substitute_parameters(context.parameters, true)?,
            None => None,
        };
        let credential_id = match credential_id {
            Some(id) => match id {
                SubstitutionResult::Single(s) => Some(s),
                SubstitutionResult::Multiple(_) => {
                    return Err("Credential ID parameter cannot be an array".to_string());
                }
            },
            None => None,
        };

        let lfs_str = match &self.lfs {
            Some(lfs) => lfs.substitute_parameters(context.parameters, false)?,
            None => return Err("Git LFS parameter must be a boolean".to_string()),
        };

        let lfs_str = match lfs_str {
            Some(lfs) => match lfs {
                SubstitutionResult::Single(s) => s,
                SubstitutionResult::Multiple(_) => {
                    return Err("Git LFS parameter cannot be an array".to_string());
                }
            },
            None => return Err("Git LFS parameter must be a boolean".to_string()),
        };

        let lfs = match lfs_str.to_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return Err("Git LFS parameter must be a boolean".to_string()),
        };

        tokio::task::yield_now().await;
        git_pull(&directory, lfs, credential_id.as_deref(), context).await
    }
}
