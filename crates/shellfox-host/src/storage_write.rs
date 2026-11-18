use crate::{
    session_facts,
    storage::{encode, storage_error, Data},
    Result,
};
use rusqlite::Connection;

pub(crate) fn save(db: &Connection, data: &Data) -> Result<()> {
    data.validate()?;
    db.execute_batch("DELETE FROM session_creation_facts; DELETE FROM requests; DELETE FROM tabs; DELETE FROM sessions;")
        .map_err(storage_error)?;
    for session in &data.sessions {
        let mut metadata = session.clone();
        metadata.tabs.clear();
        db.execute(
            "INSERT INTO sessions VALUES(?,?)",
            rusqlite::params![session.id.to_string(), encode(&metadata)?],
        )
        .map_err(storage_error)?;
        for tab in &session.tabs {
            db.execute(
                "INSERT INTO tabs VALUES(?,?,?,?)",
                rusqlite::params![
                    tab.id.to_string(),
                    session.id.to_string(),
                    tab.ordinal,
                    encode(tab)?
                ],
            )
            .map_err(storage_error)?;
        }
    }
    for (request, session) in &data.requests {
        db.execute(
            "INSERT INTO requests VALUES(?,?)",
            rusqlite::params![request.to_string(), session.to_string()],
        )
        .map_err(storage_error)?;
    }
    session_facts::save(db, &data.creation_facts)?;
    db.execute(
        "UPDATE host_state SET value=? WHERE id=1",
        [encode(&data.state)?],
    )
    .map_err(storage_error)?;
    Ok(())
}
