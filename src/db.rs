use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env;
use std::time::Duration;

use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;

pub async fn init_db_pool() -> PgPool {
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set").trim().to_string();

    // Disable statement caching. Supabase is far away, so preparing statements manually 
    // causes 4-5 extra roundtrips per query, leading to 2-3 seconds of latency.
    let mut options = PgConnectOptions::from_str(&db_url).expect("Invalid DATABASE_URL");
    options = options.statement_cache_capacity(0);

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(10) // 10 hot connections ready for concurrent API requests
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600)) // Keep connections alive longer
        .max_lifetime(Duration::from_secs(1800))
        .connect_lazy_with(options);

    pool
}
