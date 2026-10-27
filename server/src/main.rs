use std::{env, net::IpAddr};
use tokio_postgres::config::Host;
use trackfast_api::{AppState, router};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "API startup failed. Check the authentication configuration and local Postgres service."
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let google_mode = match env::var("TRACKFAST_AUTH_MODE").as_deref() {
        Ok("google") => true,
        Ok("development") | Err(_) => false,
        _ => return Err("Unknown authentication mode.".into()),
    };
    if google_mode {
        if env::var("TRACKFAST_LOCAL_MODE").as_deref() != Ok("1")
            || env::var_os("TRACKFAST_DEV_TOKEN").is_some()
            || env::var_os("TRACKFAST_DEV_MODE").is_some()
        {
            return Err("Google mode requires local mode without development credentials.".into());
        }
    } else if env::var("TRACKFAST_DEV_MODE").as_deref() != Ok("1") {
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
    let state = if google_mode {
        AppState::google(config, env::var("TRACKFAST_GOOGLE_CLIENT_ID")?)
            .map_err(|_| "Google configuration is invalid.")?
    } else {
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
    };
    state
        .migrate()
        .await
        .map_err(|_| "Database migration failed.")?;
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [command, subject, tenant, person, role]
            if google_mode && command == "provision-google-member" =>
        {
            let administrator = match role.as_str() {
                "member" => false,
                "admin" => true,
                _ => return Err("Use member or admin.".into()),
            };
            state
                .provision_google_member(
                    subject,
                    Uuid::parse_str(tenant)?,
                    Uuid::parse_str(person)?,
                    administrator,
                )
                .await
                .map_err(|_| "Membership provisioning failed.")?;
            println!(
                "Membership is present. Existing roles and disabled records remain unchanged."
            );
            return Ok(());
        }
        [command, subject] if google_mode && command == "disable-google-member" => {
            state
                .disable_google_member(subject)
                .await
                .map_err(|_| "Membership disable failed.")?;
            println!("The member is disabled. Existing sessions are revoked.");
            return Ok(());
        }
        _ => return Err("Invalid command arguments.".into()),
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8788").await?;
    println!(
        "Local API: http://127.0.0.1:8788. Authentication mode: {}. Shared hosting is not enabled.",
        if google_mode { "Google" } else { "development" }
    );
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
