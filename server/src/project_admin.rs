use crate::{Actor, ApiError, AppState, Reply, bad_request, internal, invalid, missing, pages};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectWrite {
    name: String,
    colour: String,
    archived: bool,
    member_ids: Vec<Uuid>,
}

fn body(bytes: &[u8], create: bool) -> Result<(Option<Uuid>, ProjectWrite), ApiError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| bad_request())?;
    let fields = value.as_object_mut().ok_or_else(invalid)?;
    let id = if create {
        Some(
            serde_json::from_value(fields.remove("project_id").ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
        )
    } else {
        None
    };
    let mut body: ProjectWrite = serde_json::from_value(value).map_err(|_| invalid())?;
    if body.name.trim().is_empty()
        || body.name.chars().count() > 200
        || body.colour.len() != 7
        || !body.colour.starts_with('#')
        || !body.colour.as_bytes()[1..]
            .iter()
            .all(u8::is_ascii_hexdigit)
        || body.member_ids.len() > 1000
    {
        return Err(invalid());
    }
    body.name = body.name.trim().into();
    body.colour.make_ascii_lowercase();
    body.member_ids.sort();
    if body.member_ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(invalid());
    }
    Ok((id, body))
}

pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Reply, ApiError> {
    let actor = state.administrator(&headers)?;
    let (id, body) = body(&bytes, true)?;
    mutate(&state, actor, &headers, id.unwrap(), body, None).await
}
pub(crate) async fn replace(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Reply, ApiError> {
    let actor = state.administrator(&headers)?;
    let id = Uuid::parse_str(&id).map_err(|_| invalid())?;
    let (_, body) = body(&bytes, false)?;
    mutate(
        &state,
        actor,
        &headers,
        id,
        body,
        Some(crate::entries::precondition(&headers)?),
    )
    .await
}

async fn mutate(
    state: &AppState,
    actor: Actor,
    headers: &HeaderMap,
    id: Uuid,
    body: ProjectWrite,
    expected: Option<i64>,
) -> Result<Reply, ApiError> {
    let key = crate::entries::operation_key(headers)?;
    let request = json!({"resource":"project","project_id":id,"body":body,"version":expected});
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    if let Some(reply) = crate::operations::replay(&tx, actor, key, &request).await? {
        return Ok(reply);
    }
    if body.member_ids.iter().any(|person| {
        !state
            .credentials
            .values()
            .any(|a| a.tenant == actor.tenant && a.person == *person)
    }) {
        return Err(invalid());
    }
    let row = tx
        .query_opt(
            "SELECT version FROM api_projects WHERE tenant_id=$1 AND project_id=$2 FOR UPDATE",
            &[&actor.tenant, &id],
        )
        .await
        .map_err(internal)?;
    let version = if let Some(expected) = expected {
        let current: i64 = row.ok_or_else(missing)?.get(0);
        if expected != current {
            return Err(ApiError(
                StatusCode::PRECONDITION_FAILED,
                "version_conflict",
                "The project changed. Retrieve its current version.",
            ));
        }
        let version = current.checked_add(1).ok_or_else(|| internal(()))?;
        tx.execute("UPDATE api_projects SET name=$3,colour=$4,archived=$5,version=$6 WHERE tenant_id=$1 AND project_id=$2", &[&actor.tenant,&id,&body.name,&body.colour,&body.archived,&version]).await.map_err(internal)?;
        version
    } else {
        let count = tx.execute("INSERT INTO api_projects(tenant_id,project_id,name,colour,archived) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&actor.tenant,&id,&body.name,&body.colour,&body.archived]).await.map_err(internal)?;
        if count == 0 {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "This project ID was already used.",
            ));
        }
        1
    };
    tx.execute(
        "DELETE FROM api_project_members WHERE tenant_id=$1 AND project_id=$2",
        &[&actor.tenant, &id],
    )
    .await
    .map_err(internal)?;
    for person in &body.member_ids {
        tx.execute(
            "INSERT INTO api_project_members VALUES($1,$2,$3)",
            &[&actor.tenant, &id, person],
        )
        .await
        .map_err(internal)?;
    }
    let mut data = serde_json::to_value(body).map_err(internal)?;
    data["project_id"] = json!(id);
    data["version"] = json!(version);
    tx.execute("INSERT INTO api_project_audit(tenant_id,person_id,project_id,operation_id,data) VALUES($1,$2,$3,$4,$5)", &[&actor.tenant,&actor.person,&id,&key,&data]).await.map_err(internal)?;
    let reply = Reply {
        status: if expected.is_none() { 201 } else { 200 },
        body: Some(data),
        etag: Some(format!("\"{version}\"")),
        location: expected
            .is_none()
            .then(|| format!("/v1/admin/projects/{id}")),
    };
    crate::operations::record(&tx, actor, key, &reply).await?;
    tx.commit().await.map_err(internal)?;
    Ok(reply)
}

async fn data(tx: &Transaction<'_>, actor: Actor, id: Uuid) -> Result<Value, ApiError> {
    let row = tx.query_opt("SELECT name,colour,archived,version FROM api_projects WHERE tenant_id=$1 AND project_id=$2 FOR SHARE", &[&actor.tenant,&id]).await.map_err(internal)?.ok_or_else(missing)?;
    let members: Vec<Uuid> = tx.query("SELECT person_id FROM api_project_members WHERE tenant_id=$1 AND project_id=$2 ORDER BY person_id", &[&actor.tenant,&id]).await.map_err(internal)?.iter().map(|r|r.get(0)).collect();
    Ok(
        json!({"project_id":id,"name":row.get::<_,String>(0),"colour":row.get::<_,String>(1),"archived":row.get::<_,bool>(2),"version":row.get::<_,i64>(3),"member_ids":members}),
    )
}
pub(crate) async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Reply, ApiError> {
    let actor = state.administrator(&headers)?;
    let id = Uuid::parse_str(&id).map_err(|_| invalid())?;
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    let data = data(&tx, actor, id).await?;
    let etag = Some(format!("\"{}\"", data["version"]));
    tx.commit().await.map_err(internal)?;
    Ok(Reply {
        status: 200,
        body: Some(data),
        etag,
        location: None,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    limit: Option<u16>,
    cursor: Option<Uuid>,
}
pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Reply, ApiError> {
    let actor = state.administrator(&headers)?;
    let Query(query) = query.map_err(|_| invalid())?;
    let limit = pages::limit(query.limit)?;
    let filters = json!({"limit":limit});
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    let after: Option<Uuid> = pages::position(&tx, actor, query.cursor, "admin-projects", &filters)
        .await?
        .map(serde_json::from_value)
        .transpose()
        .map_err(internal)?;
    let rows = tx.query("SELECT project_id FROM api_projects WHERE tenant_id=$1 AND ($2::uuid IS NULL OR project_id>$2) ORDER BY project_id LIMIT $3", &[&actor.tenant,&after,&(limit+1)]).await.map_err(internal)?;
    let mut items = Vec::new();
    for row in rows.iter().take(limit as usize) {
        items.push(data(&tx, actor, row.get(0)).await?);
    }
    let next = if rows.len() > limit as usize {
        Some(
            pages::save(
                &tx,
                actor,
                "admin-projects",
                &filters,
                &json!(rows[limit as usize - 1].get::<_, Uuid>(0)),
                false,
            )
            .await?,
        )
    } else {
        None
    };
    tx.commit().await.map_err(internal)?;
    Ok(Reply {
        status: 200,
        body: Some(json!({"items":items,"next_cursor":next})),
        etag: None,
        location: None,
    })
}
