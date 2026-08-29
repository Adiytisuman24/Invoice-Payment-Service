use sqlx::PgPool;
use crate::config::Config;
use crate::psp::client::PspClient;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub psp_client: PspClient,
    pub config: Config,
}
