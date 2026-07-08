use serde::{Deserialize, Serialize};

use crate::{
    log::LogLevel,
    script::{
        utils::{ParameterSubstitution, SubstitutionResult},
        ScriptExecutionContext, ScriptExecutor,
    },
    utils::execute_command,
};
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BashScript {
    pub code: String,
}

#[async_trait]
impl ScriptExecutor for BashScript {
    async fn execute(&self, context: &mut ScriptExecutionContext<'_>) -> Result<(), String> {
        // Replace all parameter references in the code
        let replaced_code = self.code.substitute_parameters(context.parameters, false)?;
        let replaced_code = match replaced_code {
            Some(code) => match code {
                SubstitutionResult::Single(s) => s,
                SubstitutionResult::Multiple(_) => {
                    return Err("Code parameter cannot be an array".to_string());
                }
            },
            None => return Ok(()),
        };

        let mut original_lines = self.code.lines();
        let lines = replaced_code.lines();
        for line in lines {
            let original_line = original_lines.next().unwrap_or("<expanded parameter line>");
            if line.is_empty() {
                continue;
            }
            tokio::task::yield_now().await;
            context
                .job_result
                .add_log(LogLevel::Info, format!("command: {}", original_line));
            if !context.job_result.dry_run {
                execute_command(line, context).await?;
            }
        }

        Ok(())
    }
}
