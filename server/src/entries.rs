use crate::{Actor, ApiError, AppState, Reply, bad_request, internal, invalid, missing};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

fn parse_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| invalid())
}

fn parse_date(raw: &str) -> Result<NaiveDate, ApiError> {
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| invalid())?;
    if date.format("%Y-%m-%d").to_string() != raw {
        return Err(invalid());
    }
    Ok(date)
}

fn entry_body(body: &[u8], create: bool) -> Result<(Option<Uuid>, Value), ApiError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| bad_request())?;
    let mut fields = value.as_object().cloned().ok_or_else(invalid)?;
    let entry_id = if create {
        Some(parse_id(
            fields
                .remove("entry_id")
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or_else(invalid)?
                .as_str(),
        )?)
    } else {
        None
    };
    let mut expected = vec!["kind", "entry_date", "project_id", "description"];
    let timed = match fields.get("kind").and_then(Value::as_str) {
        Some("duration") => false,
        Some("timed") => true,
        _ => return Err(invalid()),
    };
    if timed {
        expected.extend(["started_at", "stopped_at", "time_zone"]);
    } else {
        expected.push("original_seconds");
    }
    if fields.len() != expected.len() || expected.iter().any(|k| !fields.contains_key(*k)) {
        return Err(invalid());
    }
    if !fields["project_id"].is_null() {
        let id = parse_id(fields["project_id"].as_str().ok_or_else(invalid)?)?;
        fields.insert("project_id".into(), json!(id));
    }
    let date = fields["entry_date"].as_str().ok_or_else(invalid)?;
    let date = parse_date(date)?;
    let seconds = if timed {
        crate::timed::seconds(&fields, date)?
    } else {
        fields["original_seconds"].as_i64().ok_or_else(invalid)?
    };
    if !(1..=9_007_199_254_740_991).contains(&seconds) {
        return Err(invalid());
    }
    if fields["description"]
        .as_str()
        .ok_or_else(invalid)?
        .chars()
        .count()
        > 2000
    {
        return Err(invalid());
    }
    fields.insert("original_seconds".into(), json!(seconds));
    Ok((entry_id, Value::Object(fields)))
}

pub(crate) fn operation_key(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(bad_request)
}

pub(crate) fn precondition(headers: &HeaderMap) -> Result<i64, ApiError> {
    let raw = headers
        .get("if-match")
        .ok_or(ApiError(
            StatusCode::PRECONDITION_REQUIRED,
            "precondition_required",
            "Supply the last ETag in If-Match.",
        ))?
        .to_str()
        .map_err(|_| bad_request())?;
    let number = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(bad_request)?;
    let version: i64 = number.parse().map_err(|_| bad_request())?;
    if version <= 0 || number != version.to_string() {
        return Err(bad_request());
    }
    Ok(version)
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let (id, body) = entry_body(&bytes, true)?;
    mutate(
        &state,
        actor,
        &headers,
        id.unwrap(),
        "create",
        Some(body),
        None,
    )
    .await
}

pub(crate) async fn replace(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let (_, body) = entry_body(&bytes, false)?;
    mutate(
        &state,
        actor,
        &headers,
        parse_id(&id)?,
        "replace",
        Some(body),
        Some(precondition(&headers)?),
    )
    .await
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    mutate(
        &state,
        actor,
        &headers,
        parse_id(&id)?,
        "delete",
        None,
        Some(precondition(&headers)?),
    )
    .await
}

async fn mutate(
    state: &AppState,
    actor: Actor,
    headers: &HeaderMap,
    id: Uuid,
    action: &str,
    body: Option<Value>,
    expected: Option<i64>,
) -> Result<Reply, ApiError> {
    let key = operation_key(headers)?;
    let request = json!({"action":action,"entry_id":id,"body":body,"version":expected});
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    if let Some(reply) = crate::operations::replay(&tx, actor, key, &request).await? {
        return Ok(reply);
    }
    let saved = tx.query_opt("SELECT version,deleted,data FROM api_entries WHERE tenant_id=$1 AND person_id=$2 AND entry_id=$3 FOR UPDATE", &[&actor.tenant,&actor.person,&id]).await.map_err(internal)?;
    let version = if action == "create" {
        if saved.is_some() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "This entry ID was already used.",
            ));
        }
        1
    } else {
        let row = saved.as_ref().ok_or_else(missing)?;
        if row.get::<_, bool>(1) {
            return Err(missing());
        }
        let version: i64 = row.get(0);
        if expected != Some(version) {
            return Err(ApiError(
                StatusCode::PRECONDITION_FAILED,
                "version_conflict",
                "The entry changed. Retrieve its current version.",
            ));
        }
        version.checked_add(1).ok_or_else(|| internal(()))?
    };
    if action != "delete" {
        let previous = saved.as_ref().map(|row| row.get::<_, Value>(2));
        crate::projects::check_entry(&tx, actor, body.as_ref().unwrap(), previous.as_ref()).await?;
    }
    let interval = if action == "create" {
        crate::settings::current(&tx, actor).await?.1["rounding_interval_minutes"].clone()
    } else {
        saved.as_ref().unwrap().get::<_, Value>(2)["rounding_interval_minutes"].clone()
    };
    let mut data = body.unwrap_or_else(|| saved.as_ref().unwrap().get(2));
    data["entry_id"] = json!(id);
    data["person_id"] = json!(actor.person);
    data["version"] = json!(version);
    data["effective_seconds"] = json!(crate::settings::effective_seconds(
        data["original_seconds"]
            .as_i64()
            .ok_or_else(|| internal(()))?,
        &interval
    )?);
    data["rounding_interval_minutes"] = interval;
    data["export_state"] = json!("not_exported");
    data["updated_at"] = json!(Utc::now().to_rfc3339());
    if action == "create" {
        let inserted = tx.execute("INSERT INTO api_entries(tenant_id,person_id,entry_id,version,data) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&actor.tenant,&actor.person,&id,&version,&data]).await.map_err(internal)?;
        if inserted == 0 {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "This entry ID was already used.",
            ));
        }
    } else {
        tx.execute("UPDATE api_entries SET version=$4,deleted=$5,data=$6 WHERE tenant_id=$1 AND person_id=$2 AND entry_id=$3", &[&actor.tenant,&actor.person,&id,&version,&(action=="delete"),&data]).await.map_err(internal)?;
    }
    tx.execute("INSERT INTO api_audit(tenant_id,person_id,entry_id,operation_id,action,data) VALUES ($1,$2,$3,$4,$5,$6)", &[&actor.tenant,&actor.person,&id,&key,&action,&data]).await.map_err(internal)?;
    let reply = Reply {
        status: match action {
            "create" => 201,
            "delete" => 204,
            _ => 200,
        },
        body: if action == "delete" { None } else { Some(data) },
        etag: if action == "delete" {
            None
        } else {
            Some(format!("\"{version}\""))
        },
        location: if action == "create" {
            Some(format!("/v1/me/entries/{id}"))
        } else {
            None
        },
    };
    crate::operations::record(&tx, actor, key, &reply).await?;
    tx.commit().await.map_err(internal)?;
    Ok(reply)
}

pub(crate) async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let id = parse_id(&id)?;
    let client = state.connect().await?;
    let row = client.query_opt("SELECT data,version FROM api_entries WHERE tenant_id=$1 AND person_id=$2 AND entry_id=$3 AND NOT deleted", &[&actor.tenant,&actor.person,&id]).await.map_err(internal)?.ok_or_else(missing)?;
    Ok(Reply {
        status: 200,
        body: Some(row.get(0)),
        etag: Some(format!("\"{}\"", row.get::<_, i64>(1))),
        location: None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    from_date: String,
    to_date: String,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let Query(query) = query.map_err(|_| invalid())?;
    let from = parse_date(&query.from_date)?;
    let to = parse_date(&query.to_date)?;
    if from > to {
        return Err(invalid());
    }
    let client = state.connect().await?;
    let rows = client.query("SELECT data FROM api_entries WHERE tenant_id=$1 AND person_id=$2 AND NOT deleted AND data->>'entry_date' >= $3 AND data->>'entry_date' <= $4 ORDER BY entry_id LIMIT 101", &[&actor.tenant,&actor.person,&query.from_date,&query.to_date]).await.map_err(internal)?;
    if rows.len() > 100 {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            "Use a smaller date range. Pagination is not implemented in this development slice.",
        ));
    }
    Ok(Reply {
        status: 200,
        body: Some(
            json!({"items":rows.iter().map(|r|r.get::<_,Value>(0)).collect::<Vec<_>>(),"next_cursor":null}),
        ),
        etag: None,
        location: None,
    })
}
