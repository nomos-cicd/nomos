use std::{path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{job::JobResult, log::LogLevel};

#[derive(Deserialize, Serialize, Clone, PartialEq, Default, Debug)]
pub struct TextCredentialParameter {
    pub value: String,
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Default, Debug)]
pub struct SshCredentialParameter {
    pub username: String,
    pub private_key: String,
}

/// Similar to node.js's `.env` file.
#[derive(Deserialize, Serialize, Clone, PartialEq, Default, Debug)]
pub struct EnvCredentialParameter {
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum CredentialType {
    #[serde(rename = "text")]
    Text(TextCredentialParameter),
    #[serde(rename = "ssh")]
    Ssh(SshCredentialParameter),
    #[serde(rename = "env")]
    Env(EnvCredentialParameter),
}

impl FromStr for CredentialType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(CredentialType::Text(TextCredentialParameter::default())),
            "ssh" => Ok(CredentialType::Ssh(SshCredentialParameter::default())),
            "env" => Ok(CredentialType::Env(EnvCredentialParameter::default())),
            _ => Err(format!("Unknown credential type: {}", s)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Credential {
    pub id: String,
    pub value: CredentialType,
    pub read_only: bool,
}

impl PartialEq for Credential {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.value == other.value && self.read_only == other.read_only
    }
}

impl Credential {
    pub fn get(credential_id: &str, job_result: Option<&mut JobResult>) -> Result<Option<Self>, String> {
        let path = default_credentials_location()?.join(format!("{}.yml", credential_id));
        if !path.exists() {
            return Ok(None);
        }

        match Credential::try_from(path) {
            Ok(credential) => {
                if let Some(job_result) = job_result {
                    match &credential.value {
                        CredentialType::Text(text) => {
                            if text.value.is_empty() {
                                job_result
                                    .add_log(LogLevel::Warning, format!("Empty text credential: {}", credential_id));
                            }
                        }
                        CredentialType::Env(env) => {
                            if env.value.is_empty() {
                                job_result
                                    .add_log(LogLevel::Warning, format!("Empty env credential: {}", credential_id));
                            }
                        }
                        CredentialType::Ssh(ssh) => {
                            if ssh.username.is_empty() || ssh.private_key.is_empty() {
                                job_result
                                    .add_log(LogLevel::Warning, format!("Empty ssh credential: {}", credential_id));
                            }
                        }
                    }
                }
                Ok(Some(credential))
            }
            Err(e) => Err(e),
        }
    }

    pub fn get_all() -> Result<Vec<Self>, String> {
        let path = default_credentials_location()?;
        let mut credentials = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            match Credential::try_from(path) {
                Ok(credential) => credentials.push(credential),
                Err(e) => eprintln!("Error reading credential: {:?}", e),
            }
        }
        Ok(credentials)
    }

    pub fn get_credential_type(&self) -> &str {
        match self.value {
            CredentialType::Text(_) => "text",
            CredentialType::Ssh(_) => "ssh",
            CredentialType::Env(_) => "env",
        }
    }

    // If job_result is null, it means we are doing from the API. Allow it.
    // If job_result is not null, it means we are doing from the job. Check if the credential is changed.
    pub fn sync(&self, job_result: &mut Option<&mut JobResult>) -> Result<(), String> {
        if job_result.is_none() {
            eprintln!("Syncing credential {:?}", self.id);
            self.save()?;
            return Ok(());
        }
        let Some(job_result) = job_result.as_deref_mut() else {
            self.save()?;
            return Ok(());
        };

        let current_type = self.get_credential_type();
        let existing_credential = Credential::get(self.id.as_str(), Some(job_result))?;
        match existing_credential {
            Some(existing_credential) => {
                let existing_type = existing_credential.get_credential_type();
                if *existing_type != *current_type || self.read_only != existing_credential.read_only {
                    self.save()?;
                    job_result.add_log(LogLevel::Info, format!("Updated credential {:?}", self.id))
                } else {
                    job_result.add_log(LogLevel::Info, format!("No changes in credential {:?}", self.id))
                }
            }
            None => {
                self.save()?;
                job_result.add_log(LogLevel::Info, format!("Created credential {:?}", self.id))
            }
        }

        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let path = default_credentials_location()?.join(format!("{}.yml", self.id));
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let writer = std::io::BufWriter::new(file);
        serde_yaml::to_writer(writer, self).map_err(|e| e.to_string())
    }

    pub fn delete(&self) -> Result<(), String> {
        let path = default_credentials_location()?.join(format!("{}.yml", self.id));
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

impl TryFrom<PathBuf> for Credential {
    type Error = String;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_yaml::from_str(&content).map_err(|e| e.to_string())
    }
}

pub fn default_credentials_location() -> Result<PathBuf, String> {
    let path = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").map_err(|e| e.to_string())?;
        PathBuf::from(appdata).join("nomos").join("credentials")
    } else {
        PathBuf::from("/var/lib/nomos/credentials")
    };
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}
