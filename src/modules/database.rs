use crate::repositories::database::Database;
use crate::repositories::entities::User;
use crate::AppResult;
use axum::async_trait;
use sqlx::{Database as SqlxDatabase, Pool};

#[async_trait]
impl<DB: SqlxDatabase> Database for Pool<DB> {
    async fn login(&self, _username: String, _password: String) -> AppResult<User> {
        _ = self.acquire().await.expect("");
        Ok(User::default())
    }

    async fn logout(&self) -> AppResult<()> {
        Ok(())
    }
}
