use crate::{
    Actor, ApiError, AppState, Reply, bad_request,
    entries::{operation_key, precondition},
    internal, operations,
};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use tokio_postgres::Transaction;

fn invalid_settings() -> ApiError {
    ApiError(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Check the rounding interval, weekly target, and visual indicator.",
    )
}

fn body(bytes: &[u8]) -> Result<Value, ApiError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| bad_request())?;
    let fields = value.as_object().ok_or_else(invalid_settings)?;
    let expected = [
        "rounding_interval_minutes",
        "weekly_target_hours",
        "visual_indicator",
    ];
    if fields.len() != expected.len() || expected.iter().any(|key| !fields.contains_key(*key)) {
        return Err(invalid_settings());
    }
    if !(value["rounding_interval_minutes"].is_null()
        || value["rounding_interval_minutes"].as_u64() == Some(5))
        || !value["weekly_target_hours"]
            .as_f64()
            .is_some_and(|hours| hours.is_finite() && hours > 0.0)
        || !matches!(
            value["visual_indicator"].as_str(),
            Some("progress" | "glass")
        )
    {
        return Err(invalid_settings());
    }
    Ok(value)
}

pub(crate) async fn current(tx: &Transaction<'_>, actor: Actor) -> Result<(i64, Value), ApiError> {
    let defaults = json!({"rounding_interval_minutes":5,"weekly_target_hours":35,"visual_indicator":"progress"});
    tx.execute("INSERT INTO api_settings(tenant_id,person_id,version,data) VALUES ($1,$2,1,$3) ON CONFLICT DO NOTHING", &[&actor.tenant,&actor.person,&defaults]).await.map_err(internal)?;
    let row = tx
        .query_one(
            "SELECT version,data FROM api_settings WHERE tenant_id=$1 AND person_id=$2 FOR UPDATE",
            &[&actor.tenant, &actor.person],
        )
        .await
        .map_err(internal)?;
    Ok((row.get(0), row.get(1)))
}

fn reply(version: i64, mut data: Value) -> Reply {
    data["version"] = json!(version);
    Reply {
        status: 200,
        body: Some(data),
        etag: Some(format!("\"{version}\"")),
        location: None,
    }
}

pub(crate) async fn read(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    let (version, data) = current(&tx, actor).await?;
    tx.commit().await.map_err(internal)?;
    Ok(reply(version, data))
}

pub(crate) async fn replace(
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let key = operation_key(&headers)?;
    let expected = precondition(&headers)?;
    let data = body(&bytes)?;
    let request = json!({"action":"replace_settings","body":data,"version":expected});
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    if let Some(reply) = operations::replay(&tx, actor, key, &request).await? {
        return Ok(reply);
    }
    let (version, _) = current(&tx, actor).await?;
    if expected != version {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "version_conflict",
            "The settings changed. Retrieve their current version.",
        ));
    }
    let version = version.checked_add(1).ok_or_else(|| internal(()))?;
    tx.execute(
        "UPDATE api_settings SET version=$3,data=$4 WHERE tenant_id=$1 AND person_id=$2",
        &[&actor.tenant, &actor.person, &version, &data],
    )
    .await
    .map_err(internal)?;
    let reply = reply(version, data);
    tx.execute("INSERT INTO api_settings_audit(tenant_id,person_id,operation_id,data) VALUES ($1,$2,$3,$4)", &[&actor.tenant,&actor.person,&key,&reply.body]).await.map_err(internal)?;
    operations::record(&tx, actor, key, &reply).await?;
    tx.commit().await.map_err(internal)?;
    Ok(reply)
}

pub(crate) fn effective_seconds(seconds: i64, interval: &Value) -> Result<i64, ApiError> {
    match interval {
        Value::Null => Ok(seconds),
        value if value.as_u64() == Some(5) => Ok(((seconds + 150) / 300) * 300),
        _ => Err(internal(())),
    }
}
