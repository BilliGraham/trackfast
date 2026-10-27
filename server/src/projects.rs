use crate::{Actor, ApiError, AppState, Reply, internal, invalid};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn check_entry(
    tx: &Transaction<'_>,
    actor: Actor,
    data: &Value,
    previous: Option<&Value>,
) -> Result<(), ApiError> {
    if data["project_id"].is_null() {
        return Ok(());
    }
    let id: Uuid = data["project_id"]
        .as_str()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let row=tx.query_opt("SELECT p.archived FROM api_projects p JOIN api_project_members m USING (tenant_id,project_id) WHERE p.tenant_id=$1 AND p.project_id=$2 AND m.person_id=$3 FOR SHARE OF p,m", &[&actor.tenant,&id,&actor.person]).await.map_err(internal)?.ok_or_else(invalid)?;
    if row.get::<_, bool>(0) {
        let workload = [
            "project_id",
            "kind",
            "entry_date",
            "original_seconds",
            "started_at",
            "stopped_at",
            "time_zone",
        ];
        if !previous.is_some_and(|old| workload.iter().all(|key| old.get(*key) == data.get(*key))) {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    limit: Option<u16>,
    cursor: Option<Uuid>,
}

fn invalid_page() -> ApiError {
    ApiError(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Check the page size and cursor. Restart the project list if the cursor expired.",
    )
}

pub(crate) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Reply, ApiError> {
    let actor = state.actor(&headers).await?;
    let Query(query) = query.map_err(|_| invalid_page())?;
    let limit = i64::from(query.limit.unwrap_or(50));
    if !(1..=100).contains(&limit) {
        return Err(invalid_page());
    }
    let mut client = state.connect().await?;
    let tx = client.transaction().await.map_err(internal)?;
    tx.execute(
        "DELETE FROM api_project_cursors WHERE expires_at <= now()",
        &[],
    )
    .await
    .map_err(internal)?;
    let after = if let Some(cursor) = query.cursor {
        let row=tx.query_opt("SELECT after_id FROM api_project_cursors WHERE cursor_id=$1 AND tenant_id=$2 AND person_id=$3 AND page_size=$4 AND expires_at > now()", &[&cursor,&actor.tenant,&actor.person,&limit]).await.map_err(internal)?.ok_or_else(invalid_page)?;
        Some(row.get::<_, Uuid>(0))
    } else {
        None
    };
    let rows=tx.query("SELECT p.project_id,p.name,p.colour,p.archived FROM api_projects p JOIN api_project_members m USING (tenant_id,project_id) WHERE p.tenant_id=$1 AND m.person_id=$2 AND ($3::uuid IS NULL OR p.project_id > $3) ORDER BY p.project_id LIMIT $4", &[&actor.tenant,&actor.person,&after,&(limit+1)]).await.map_err(internal)?;
    let page: Vec<_>=rows.iter().take(limit as usize).map(|row|json!({"project_id":row.get::<_,Uuid>(0),"name":row.get::<_,String>(1),"colour":row.get::<_,String>(2),"archived":row.get::<_,bool>(3)})).collect();
    let next = if rows.len() > limit as usize {
        let cursor = Uuid::new_v4();
        let last: Uuid = rows[limit as usize - 1].get(0);
        tx.execute("INSERT INTO api_project_cursors(cursor_id,tenant_id,person_id,after_id,page_size) VALUES ($1,$2,$3,$4,$5)", &[&cursor,&actor.tenant,&actor.person,&last,&limit]).await.map_err(internal)?;
        Some(cursor)
    } else {
        None
    };
    tx.commit().await.map_err(internal)?;
    Ok(Reply {
        status: 200,
        body: Some(json!({"items":page,"next_cursor":next})),
        etag: None,
        location: None,
    })
}
