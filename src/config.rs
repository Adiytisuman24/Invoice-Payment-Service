use std::env;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub psp_url: String,
    pub port: u16,
    pub demo_business_id: Uuid,
    pub demo_api_key: String,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/dodo".to_string());
        
        let psp_url = env::var("PSP_URL")
            .unwrap_or_else(|_| "http://localhost:8081".to_string());
        
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        let demo_business_id = env::var("DEMO_BUSINESS_ID")
            .ok()
            .and_then(|id| Uuid::parse_str(&id).ok())
            .unwrap_or_else(|| Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());

        let demo_api_key = env::var("DEMO_API_KEY")
            .unwrap_or_else(|_| "dodo_test_seed_key_abc123".to_string());

        Self {
            database_url,
            psp_url,
            port,
            demo_business_id,
            demo_api_key,
        }
    }
}
