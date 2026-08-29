use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use tracing::info;

pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn init_db(database_url: &str) -> Result<PgPool, anyhow::Error> {
    info!("Connecting to database...");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), anyhow::Error> {
    info!("Running database migrations...");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await?;
    info!("Migrations complete.");
    Ok(())
}

pub async fn seed_demo_data(pool: &PgPool, business_id: Uuid, api_key: &str) -> Result<(), anyhow::Error> {
    info!("Seeding demo business and API key if not present...");
    
    // Insert business if not exists
    sqlx::query(
        "INSERT INTO businesses (id, name, created_at)
         VALUES ($1, $2, now())
         ON CONFLICT (id) DO NOTHING"
    )
    .bind(business_id)
    .bind("Demo Business")
    .execute(pool)
    .await?;

    // Hash the API key
    let key_hash = hash_api_key(api_key);
    let key_prefix = if api_key.len() > 10 {
        &api_key[0..10]
    } else {
        "dodo_test"
    };

    // Insert API key if not exists
    sqlx::query(
        "INSERT INTO api_keys (id, business_id, key_prefix, key_hash, created_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (key_hash) DO NOTHING"
    )
    .bind(Uuid::new_v4())
    .bind(business_id)
    .bind(key_prefix)
    .bind(key_hash)
    .execute(pool)
    .await?;

    info!("Demo seeding checked.");
    Ok(())
}
