use std::{path::PathBuf, str::FromStr, sync::Arc};

use sqlx::{
    Error, Pool, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions},
};
use utils::assets::asset_dir;

pub mod models;

#[derive(Clone)]
pub struct DBService {
    pub pool: Pool<Sqlite>,
}

impl DBService {
    /// Get the database URL from environment variable or default to asset_dir
    fn get_database_url() -> String {
        if let Ok(db_url) = std::env::var("DATABASE_URL") {
            // If DATABASE_URL is set, use it
            // Handle both absolute paths and relative paths
            if db_url.starts_with("sqlite://") {
                let path_part = db_url.strip_prefix("sqlite://").unwrap();
                if PathBuf::from(path_part).is_absolute() {
                    db_url
                } else {
                    // Relative path - resolve from current working directory
                    let abs_path = std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .join(path_part);
                    Self::format_sqlite_url(&abs_path)
                }
            } else {
                db_url
            }
        } else {
            // Default to asset_dir/db.sqlite
            let db_path = asset_dir().join("db.sqlite");
            Self::format_sqlite_url(&db_path)
        }
    }

    /// Format a path as a proper SQLite URL
    /// SQLite URL format: sqlite:// + path
    /// For absolute paths on Unix (starting with /), this results in sqlite:///path (3 slashes)
    /// For Windows paths, this results in sqlite://C:/path
    fn format_sqlite_url(path: &PathBuf) -> String {
        // Ensure the path is absolute
        let abs_path = if path.is_absolute() {
            path.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };

        let abs_path_str = abs_path.to_string_lossy();

        // SQLite URL format: sqlite:// followed by the path
        // For Unix absolute paths (/home/...), this becomes sqlite:///home/...
        // The third slash is the root directory indicator
        if abs_path_str.starts_with('/') {
            // Unix absolute path - sqlite:// + /path = sqlite:///path
            format!("sqlite://{}", abs_path_str)
        } else if abs_path_str.len() >= 2 && abs_path_str.chars().nth(1) == Some(':') {
            // Windows absolute path (C:\...) - needs special handling
            format!("sqlite:///{}", abs_path_str)
        } else {
            // Fallback - treat as relative (shouldn't happen after is_absolute check)
            format!("sqlite://{}", abs_path_str)
        }
    }

    pub async fn new() -> Result<DBService, Error> {
        let database_url = Self::get_database_url();
        let options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(DBService { pool })
    }

    pub async fn new_with_after_connect<F>(after_connect: F) -> Result<DBService, Error>
    where
        F: for<'a> Fn(
                &'a mut SqliteConnection,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>,
            > + Send
            + Sync
            + 'static,
    {
        let pool = Self::create_pool(Some(Arc::new(after_connect))).await?;
        Ok(DBService { pool })
    }

    async fn create_pool<F>(after_connect: Option<Arc<F>>) -> Result<Pool<Sqlite>, Error>
    where
        F: for<'a> Fn(
                &'a mut SqliteConnection,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>,
            > + Send
            + Sync
            + 'static,
    {
        let database_url = Self::get_database_url();
        let options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);

        let pool = if let Some(hook) = after_connect {
            SqlitePoolOptions::new()
                .after_connect(move |conn, _meta| {
                    let hook = hook.clone();
                    Box::pin(async move {
                        hook(conn).await?;
                        Ok(())
                    })
                })
                .connect_with(options)
                .await?
        } else {
            SqlitePool::connect_with(options).await?
        };

        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(pool)
    }
}
