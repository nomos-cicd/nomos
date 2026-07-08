use std::collections::HashMap;

use axum::async_trait;
use axum_login::{AuthUser, AuthnBackend, UserId};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct User {
    id: i64,
    pub username: String,
    password: String,
}

// Here we've implemented `Debug` manually to avoid accidentally logging the
// password hash.
impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl AuthUser for User {
    type Id = i64;

    fn id(&self) -> Self::Id {
        self.id
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.password.as_bytes() // We use the password hash as the auth
                                 // hash--what this means
                                 // is when the user changes their password the
                                 // auth session becomes invalid.
    }
}

#[derive(Clone, Default)]
pub struct Backend {
    #[allow(dead_code)]
    users: HashMap<i64, User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    pub next: Option<String>,
}

#[async_trait]
impl AuthnBackend for Backend {
    type User = User;
    type Credentials = Credentials;
    type Error = std::convert::Infallible;

    async fn authenticate(
        &self,
        Credentials {
            username,
            password,
            next: _,
        }: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let expected_username = match std::env::var("NOMOS_USERNAME") {
            Ok(username) => username,
            Err(e) => {
                eprintln!("NOMOS_USERNAME environment variable is not available: {}", e);
                return Ok(None);
            }
        };
        let expected_password = match std::env::var("NOMOS_PASSWORD") {
            Ok(password) => password,
            Err(e) => {
                eprintln!("NOMOS_PASSWORD environment variable is not available: {}", e);
                return Ok(None);
            }
        };
        if username != expected_username || password != expected_password {
            return Ok(None);
        }

        let user = User {
            id: 1,
            username,
            password,
        };

        return Ok(user.into());
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let username = match std::env::var("NOMOS_USERNAME") {
            Ok(username) => username,
            Err(e) => {
                eprintln!("NOMOS_USERNAME environment variable is not available: {}", e);
                return Ok(None);
            }
        };
        let password = match std::env::var("NOMOS_PASSWORD") {
            Ok(password) => password,
            Err(e) => {
                eprintln!("NOMOS_PASSWORD environment variable is not available: {}", e);
                return Ok(None);
            }
        };

        return Ok(Some(Self::User {
            id: *user_id,
            username,
            password,
        }));
    }
}
