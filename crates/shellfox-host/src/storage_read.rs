//! Reads the persisted store into the Host's working Data.
use crate::{
    storage::{not_found, storage_error, Data},
    Result, SessionCreationFactMap,
};
use rusqlite::Connection;
use shellfox_core::*;

fn decode<T: serde::de::DeserializeOwned>(value: String) -> Result<T> {
    if value.len() > 16 * 1024 * 1024 {
        return Err(storage_error("oversized"));
    }
    serde_json::from_str(&value)
        .map_err(|_| Error::new(ErrorCode::StorageFailed, "Saved metadata is invalid."))
}
pub(crate) fn read(db: &Connection) -> Result<Data> {
    let tx = db.unchecked_transaction().map_err(storage_error)?;
    let data = load(&tx)?;
    tx.commit().map_err(storage_error)?;
    Ok(data)
}
pub(crate) fn load(db: &Connection) -> Result<Data> {
    let state = decode(
        db.query_row("SELECT value FROM host_state WHERE id=1", [], |r| r.get(0))
            .map_err(storage_error)?,
    )?;
    let mut sessions = Vec::<Session>::new();
    let mut stmt = db
        .prepare("SELECT id,value FROM sessions ORDER BY id")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage_error)?
    {
        let (id, value) = row.map_err(storage_error)?;
        let session: Session = decode(value)?;
        validation::ensure(
            session.id.to_string() == id && session.tabs.is_empty(),
            "Invalid saved session identity.",
        )?;
        sessions.push(session);
    }
    let mut stmt = db
        .prepare("SELECT id,session_id,ordinal,value FROM tabs ORDER BY session_id,ordinal")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(storage_error)?
    {
        let (id, parent, ordinal, value) = row.map_err(storage_error)?;
        let tab: Tab = decode(value)?;
        validation::ensure(
            tab.id.to_string() == id
                && tab.session_id.to_string() == parent
                && tab.ordinal == ordinal,
            "Invalid saved tab identity.",
        )?;
        sessions
            .iter_mut()
            .find(|s| s.id == tab.session_id)
            .ok_or_else(not_found)?
            .tabs
            .push(tab);
    }
    let mut requests = Vec::new();
    let mut stmt = db
        .prepare("SELECT request_id,session_id FROM requests")
        .map_err(storage_error)?;
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(storage_error)?
    {
        let (a, b) = row.map_err(storage_error)?;
        requests.push((a.parse()?, b.parse()?));
    }
    let version: i32 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage_error)?;
    let creation_facts = if version >= 3 {
        crate::session_facts::read(db)?
    } else {
        SessionCreationFactMap::new()
    };
    let mut data = Data {
        state,
        sessions,
        requests,
        creation_facts,
    };
    if version >= 2 {
        crate::settings_store::project(&crate::settings_store::read(db)?, &mut data)?;
    }
    data.validate()
        .map_err(|_| storage_error("invalid persisted state"))?;
    if version >= 4 {
        crate::reserved_publication::validate_store(db, &data)?;
    }
    Ok(data)
}
