use super::*;
use trackfast_sync::Scope;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Enrollment {
    pub scope: Scope,
    pub boundary: i64,
    pub mappings: Vec<(i64, Uuid)>,
    pub new_entries: i64,
}

fn read_binding(connection: &Connection) -> Result<Option<(Scope, i64)>> {
    let row: Option<(String, String, String, i64)> = connection
        .query_row(
            "SELECT server,tenant_id,person_id,boundary FROM sync_enrollment WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(server, tenant, person, boundary)| {
        Ok((
            Scope {
                server,
                tenant_id: parse_id(&tenant)?,
                person_id: parse_id(&person)?,
            },
            boundary,
        ))
    })
    .transpose()
}

fn parse_id(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error("The saved sync identity is invalid.".into()))
}

pub(super) fn require_scope(connection: &Connection, scope: &Scope) -> Result<()> {
    match read_binding(connection)? {
        Some((saved, _)) if saved == *scope => Ok(()),
        _ => Err(Error(
            "Sync setup belongs to another account or is not complete.".into(),
        )),
    }
}

impl Store {
    pub fn enrollment(&self) -> Result<Option<Enrollment>> {
        let Some((scope, boundary)) = read_binding(&self.connection)? else {
            return Ok(None);
        };
        let mut statement = self.connection.prepare("SELECT local_project_id,remote_project_id FROM sync_project_mappings ORDER BY local_project_id")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut mappings = Vec::new();
        for row in rows {
            let (local, remote) = row?;
            mappings.push((local, parse_id(&remote)?));
        }
        let mut enrollment = Enrollment {
            scope,
            boundary,
            mappings,
            new_entries: 0,
        };
        enrollment.new_entries = self.connection.query_row(
            "SELECT COUNT(*) FROM session_sync_identity i JOIN sessions s ON s.id=i.session_id WHERE i.sequence>?1 AND s.stop_utc IS NOT NULL AND s.toggl_id IS NULL",
            [enrollment.boundary], |r| r.get(0),
        )?;
        Ok(Some(enrollment))
    }

    pub fn enroll(&mut self, scope: &Scope) -> Result<()> {
        if scope.tenant_id.is_nil()
            || scope.person_id.is_nil()
            || trackfast_sync::transport::DevelopmentClient::new(
                scope.clone(),
                "configuration-validation-only-token",
            )
            .is_err()
        {
            return Err(Error("The sync account configuration is invalid.".into()));
        }
        let tx = self.connection.transaction()?;
        if read_binding(&tx)?.is_some() {
            require_scope(&tx, scope)?;
        } else {
            tx.execute("INSERT INTO sync_enrollment(id,server,tenant_id,person_id,boundary) SELECT 1,?1,?2,?3,COALESCE(MAX(sequence),0) FROM session_sync_identity",
                params![scope.server, scope.tenant_id.to_string(), scope.person_id.to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn map_project(&mut self, scope: &Scope, local: i64, remote: Uuid) -> Result<()> {
        if remote.is_nil() {
            return Err(Error("Select an API project.".into()));
        }
        let tx = self.connection.transaction()?;
        require_scope(&tx, scope)?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND archived=0)",
            [local],
            |r| r.get(0),
        )?;
        if !active {
            return Err(Error("Select an active local project.".into()));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT remote_project_id FROM sync_project_mappings WHERE local_project_id=?1",
                [local],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if parse_id(&existing)? != remote {
                return Err(Error(
                    "This project already has a mapping. Reassignment is not supported yet.".into(),
                ));
            }
        } else {
            tx.execute(
                "INSERT INTO sync_project_mappings VALUES (?1,?2)",
                params![local, remote.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
