use crate::{Actor, ApiError, Reply, internal};
use axum::http::StatusCode;
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn replay(
    tx: &Transaction<'_>,
    actor: Actor,
    key: Uuid,
    request: &Value,
) -> Result<Option<Reply>, ApiError> {
    tx.execute("INSERT INTO api_operations(tenant_id,person_id,operation_id,request) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING", &[&actor.tenant,&actor.person,&key,request]).await.map_err(internal)?;
    let operation = tx.query_one("SELECT request,response FROM api_operations WHERE tenant_id=$1 AND person_id=$2 AND operation_id=$3 FOR UPDATE", &[&actor.tenant,&actor.person,&key]).await.map_err(internal)?;
    if operation.get::<_, Value>(0) != *request {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "Use the same operation key only for the same request.",
        ));
    }
    operation
        .get::<_, Option<Value>>(1)
        .map(serde_json::from_value)
        .transpose()
        .map_err(internal)
}

pub(crate) async fn record(
    tx: &Transaction<'_>,
    actor: Actor,
    key: Uuid,
    reply: &Reply,
) -> Result<(), ApiError> {
    tx.execute("UPDATE api_operations SET response=$4 WHERE tenant_id=$1 AND person_id=$2 AND operation_id=$3", &[&actor.tenant,&actor.person,&key,&serde_json::to_value(reply).map_err(internal)?]).await.map_err(internal)?;
    Ok(())
}
