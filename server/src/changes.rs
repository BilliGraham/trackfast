use crate::{Actor, ApiError, AppState, Reply, internal, invalid, pages};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn lock(tx: &Transaction<'_>, actor: Actor) -> Result<(), ApiError> {
    tx.execute(
        "INSERT INTO api_entry_clocks(tenant_id,person_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        &[&actor.tenant, &actor.person],
    )
    .await
    .map_err(internal)?;
    // The account lock keeps change revisions in commit order, including concurrent writes.
    tx.query_one(
        "SELECT revision FROM api_entry_clocks WHERE tenant_id=$1 AND person_id=$2 FOR UPDATE",
        &[&actor.tenant, &actor.person],
    )
    .await
    .map_err(internal)?;
    Ok(())
}

pub(crate) async fn record(
    tx: &Transaction<'_>,
    actor: Actor,
    id: Uuid,
    version: i64,
    deleted: bool,
    data: &Value,
) -> Result<(), ApiError> {
    let revision: i64 = tx.query_one("UPDATE api_entry_clocks SET revision=revision+1 WHERE tenant_id=$1 AND person_id=$2 RETURNING revision", &[&actor.tenant,&actor.person]).await.map_err(internal)?.get(0);
    tx.execute(
        "INSERT INTO api_entry_changes VALUES($1,$2,$3,$4,$5,$6,$7)",
        &[
            &actor.tenant,
            &actor.person,
            &revision,
            &id,
            &version,
            &deleted,
            data,
        ],
    )
    .await
    .map_err(internal)?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeQuery {
    limit: Option<u16>,
    cursor: Option<Uuid>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ChangeQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers)?;
    let Query(query) = query.map_err(|_| invalid())?;
    let limit = pages::limit(query.limit)?;
    let filters = json!({"limit":limit});
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    let position = pages::position(&tx, actor, query.cursor, "changes", &filters).await?;
    let after = position.as_ref().and_then(Value::as_i64).unwrap_or(0);
    let rows = tx.query("SELECT revision,entry_id,version,deleted,data FROM api_entry_changes WHERE tenant_id=$1 AND person_id=$2 AND revision>$3 ORDER BY revision LIMIT $4", &[&actor.tenant,&actor.person,&after,&(limit+1)]).await.map_err(internal)?;
    let items: Vec<_> = rows.iter().take(limit as usize).map(|r| {
        let deleted: bool = r.get(3);
        json!({"entry_id":r.get::<_,Uuid>(1),"version":r.get::<_,i64>(2),"deleted":deleted,"entry":if deleted {Value::Null} else {r.get(4)}})
    }).collect();
    let last = rows
        .iter()
        .take(limit as usize)
        .last()
        .map(|r| r.get::<_, i64>(0))
        .unwrap_or(after);
    let cursor = if let Some(cursor) = query.cursor.filter(|_| last == after) {
        cursor
    } else {
        pages::save(&tx, actor, "changes", &filters, &json!(last), true).await?
    };
    tx.commit().await.map_err(internal)?;
    Ok(Reply {
        status: 200,
        body: Some(
            json!({"items":items,"next_cursor":cursor,"has_more":rows.len()>limit as usize}),
        ),
        etag: None,
        location: None,
    })
}
