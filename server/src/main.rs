use std::{env, net::IpAddr};
use tokio_postgres::config::Host;
use trackfast_api::{AppState, router};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "API startup failed. Check the development configuration and local Postgres service."
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("TRACKFAST_DEV_MODE").as_deref() != Ok("1") {
        return Err("Development mode is required.".into());
    }
    let config: tokio_postgres::Config = env::var("TRACKFAST_DATABASE_URL")?.parse()?;
    if config.get_hosts().is_empty()
        || config.get_hosts().iter().any(|host| match host {
            Host::Tcp(value) => {
                value != "localhost" && !value.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
            }
            #[cfg(unix)]
            Host::Unix(_) => true,
        })
        || config.get_hostaddrs().iter().any(|ip| !ip.is_loopback())
    {
        return Err("Only loopback Postgres hosts are permitted.".into());
    }
    let token = env::var("TRACKFAST_DEV_TOKEN")?;
    if token.len() < 32 {
        return Err("The development token is too short.".into());
    }
    let tenant = Uuid::parse_str(&env::var("TRACKFAST_DEV_TENANT_ID")?)?;
    let person = Uuid::parse_str(&env::var("TRACKFAST_DEV_PERSON_ID")?)?;
    let mut state = AppState::development(config, vec![(token, tenant, person)]);
    if env::var("TRACKFAST_DEV_PROJECT_ADMIN").as_deref() == Ok("1") {
        state = state.with_development_administrators(vec![(tenant, person)]);
    }
    state
        .migrate()
        .await
        .map_err(|_| "Database migration failed.")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8788").await?;
    println!(
        "Development API: http://127.0.0.1:8788. Personal rounding defaults to five minutes. Production sign-in is not implemented."
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
