use crate::{Actor, ApiError, internal, invalid};
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) fn limit(value: Option<u16>) -> Result<i64, ApiError> {
    let limit = i64::from(value.unwrap_or(50));
    if !(1..=100).contains(&limit) {
        return Err(invalid());
    }
    Ok(limit)
}

pub(crate) async fn position(
    tx: &Transaction<'_>,
    actor: Actor,
    cursor: Option<Uuid>,
    kind: &str,
    filters: &Value,
) -> Result<Option<Value>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let row = tx.query_opt("SELECT position FROM api_page_cursors WHERE cursor_id=$1 AND tenant_id=$2 AND person_id=$3 AND kind=$4 AND filters=$5 AND (expires_at IS NULL OR expires_at > now())", &[&cursor,&actor.tenant,&actor.person,&kind,filters]).await.map_err(internal)?.ok_or_else(invalid)?;
    Ok(Some(row.get(0)))
}

pub(crate) async fn save(
    tx: &Transaction<'_>,
    actor: Actor,
    kind: &str,
    filters: &Value,
    position: &Value,
    durable: bool,
) -> Result<Uuid, ApiError> {
    tx.execute(
        "DELETE FROM api_page_cursors WHERE expires_at <= now()",
        &[],
    )
    .await
    .map_err(internal)?;
    let id = Uuid::new_v4();
    tx.execute("INSERT INTO api_page_cursors(cursor_id,tenant_id,person_id,kind,filters,position,expires_at) VALUES($1,$2,$3,$4,$5,$6,CASE WHEN $7 THEN NULL ELSE now()+interval '15 minutes' END)", &[&id,&actor.tenant,&actor.person,&kind,filters,position,&durable]).await.map_err(internal)?;
    Ok(id)
}
