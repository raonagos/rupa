use super::entities::User;
use crate::AppResult;
use axum::{async_trait, extract::FromRef};
use leptos::config::LeptosOptions;
use std::{ops::Deref, sync::Arc};

#[async_trait]
pub trait Database: std::fmt::Debug + Send + Sync {
    // session
    async fn login(&self, username: String, password: String) -> AppResult<User>;
    async fn logout(&self) -> AppResult<()>;
}

#[derive(Debug, Clone, FromRef)]
pub struct AppState {
    pub leptos_options: LeptosOptions,
    pub pool: Arc<dyn Database>,
}

// #[async_trait]
// impl<S> FromRequestParts<S> for AppState
// where
//     S: Sync + Send,
// {
//     type Rejection = (http::StatusCode, &'static str);

//     async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
//         parts.extensions.get::<Self>().cloned().ok_or((
//             http::StatusCode::INTERNAL_SERVER_ERROR,
//             "Can't extract database. Is `AppState` added ?",
//         ))
//     }
// }

impl Deref for AppState {
    type Target = dyn Database;
    fn deref(&self) -> &Self::Target {
        &*self.pool
    }
}
