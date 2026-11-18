//! Native0 owner-loop attach. Never writes recovery ANSI into another parser.
use super::{
    connection::{PeerConnection, reply, settlement},
    peer_frame, peer_keys,
};
use anyhow::{Result, anyhow, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal,
};
use shellfox_mobile::{
    ManagerOwnerView, ManagerPollContext, ManagerRecovery, ManagerRecoveryError,
};
use shellfox_protocol::{OutboundTransport, manager_v2 as m};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
type Job<'a, T> =
    Pin<Box<dyn Future<Output = std::result::Result<m::Reply<T>, ManagerRecoveryError>> + 'a>>;
fn idle<'a, T: 'a>() -> Job<'a, T> {
    Box::pin(std::future::pending())
}
fn sdk(error: ManagerRecoveryError) -> anyhow::Error {
    anyhow!("Native Manager: {error:?}")
}
struct Owner(ManagerOwnerView);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}
enum End {
    Detached,
    Exited,
    Recover,
    Reconnect,
}
fn failure(error: ManagerRecoveryError) -> Result<End> {
    use shellfox_client::manager_v2::ClientError;
    use shellfox_protocol::TransportError;
    if matches!(
        error,
        ManagerRecoveryError::Client(ClientError::Transport(
            TransportError::Unavailable | TransportError::Deadline
        ))
    ) {
        return Ok(End::Reconnect);
    }
    Err(sdk(error))
}
pub(crate) async fn run<T: OutboundTransport>(
    connection: Arc<PeerConnection<T>>,
    tab: m::Id,
    mut takeover: bool,
) -> Result<()> {
    crate::attach::require_terminal()?;
    let _terminal = crate::attach::TerminalGuard::enter()?;
    loop {
        match session(&connection, tab, takeover).await? {
            End::Detached | End::Exited => return Ok(()),
            End::Recover => {
                takeover = false;
            }
            End::Reconnect => {
                // Retire the real binding before another PAIR. No old input job,
                // ACK, grant or parsed frame crosses this connection boundary.
                connection.reconnect()?;
                eprintln!(
                    "Peer disconnected. Reconnecting; pending input is not replayed. Ctrl+] detaches."
                );
                loop {
                    match connection.connect().await {
                        Ok(_) => break,
                        Err(error)
                            if matches!(
                                connection.status(),
                                shellfox_client::manager_v2::Status::Rejected
                            ) =>
                        {
                            return Err(error);
                        }
                        Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                    }
                    if event::poll(Duration::ZERO)?
                        && matches!(event::read()?,Event::Key(key) if key.code==KeyCode::Char(']') && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Ok(());
                    }
                }
                takeover = false;
            }
        }
    }
}
async fn session<T: OutboundTransport>(
    connection: &Arc<PeerConnection<T>>,
    tab: m::Id,
    takeover: bool,
) -> Result<End> {
    let origin = connection.origin()?;
    let snapshot = connection.snapshot(&origin).await?;
    let session = snapshot
        .entries
        .iter()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } if value.tabs.iter().any(|item| item.id == tab) => {
                Some(value.as_ref())
            }
            _ => None,
        })
        .ok_or_else(|| anyhow!("Remote tab is no longer available."))?;
    let session_id = session.id;
    let control = super::connection::attach_control(&snapshot, tab)?;
    let mut settled = settlement(&snapshot, tab);
    let client = connection.client();
    let (cols, rows) = terminal::size()?;
    let mut recovery = ManagerRecovery::from_shared_native0(
        client.clone(),
        cols.max(1),
        rows.max(1),
        Default::default(),
    )
    .map_err(sdk)?;
    let owner = Owner(recovery.owner_view());
    let attached = reply(
        recovery
            .attach(m::AttachTerminal {
                tab_id: tab,
                resume: None,
                control,
            })
            .await
            .map_err(sdk)?,
    )?;
    if let m::Attachment::Unavailable { reason, .. } = attached {
        bail!("Remote terminal unavailable: {reason:?}");
    }
    owner.0.pump().map_err(sdk)?;
    reply(recovery.acknowledge_consumed().await.map_err(sdk)?)?;
    if takeover {
        reply(recovery.take_control().await.map_err(sdk)?)?;
        owner.0.pump().map_err(sdk)?;
        reply(recovery.acknowledge_consumed().await.map_err(sdk)?)?;
    }
    let recovery = Arc::new(recovery);
    let mut pending = super::peer_pending::PendingInputs::default();
    let mut input_job: Job<'_, m::AcceptedInput> = idle();
    let mut input_active = false;
    let mut ack_job: Job<'_, shellfox_protocol::Acknowledged> = idle();
    let mut ack_active = false;
    let mut resize_job: Job<'_, m::Resized> = idle();
    let mut resize_active = false;
    let mut resize = Some((cols, rows));
    let mut context = ManagerPollContext::capture(&client, &[&recovery]).map_err(sdk)?;
    let mut poll: Job<'_, m::PollResponse> = Box::pin(context.poll(&client));
    let mut previous: Option<shellfox_ghostty_vt::Screen> = None;
    let mut viewport = (cols, rows);
    let mut end_after_pump = None;
    let mut pump_epoch = 0u64;
    let mut ack_epoch = 0u64;
    let mut refresh = false;
    let mut catalog_active = false;
    let mut catalog: Pin<Box<dyn Future<Output = Result<m::Snapshot>> + '_>> =
        Box::pin(std::future::pending());
    let mut ticks = tokio::time::interval(Duration::from_millis(16));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _=ticks.tick()=>{
                owner.0.pump().map_err(sdk)?;
                pump_epoch=pump_epoch.checked_add(1).ok_or_else(||anyhow!("Native owner pump cycle exhausted."))?;
                if refresh&&!catalog_active {let connection=connection.clone();let origin=origin.clone();catalog=Box::pin(async move{connection.snapshot(&origin).await});catalog_active=true;refresh=false;}
                let frame=owner.0.frame().map_err(sdk)?;
                let changed=previous.as_ref().is_none_or(|old|old.cols!=frame.cols||old.rows!=frame.rows||old.cursor!=frame.cursor||old.cells!=frame.cells);
                if changed { peer_frame::paint(&frame,viewport,&mut std::io::stdout().lock())?; previous=Some(frame); }
                if !ack_active { let view=recovery.clone();ack_job=Box::pin(async move{view.acknowledge_consumed().await});ack_epoch=pump_epoch;ack_active=true; }
                for _ in 0..64 {
                    if !event::poll(Duration::ZERO)? { break; }
                    let input=match event::read()? {
                        Event::Resize(cols,rows)=>{viewport=(cols,rows);resize=Some(viewport);previous=None;continue;},
                        Event::Key(key) if key.code==KeyCode::Char(']')&&key.modifiers.contains(KeyModifiers::CONTROL)=>return Ok(End::Detached),
                        Event::Key(key)=>match peer_keys::key(key) { Ok(event)=>m::Input::Key{event},Err(error)=>{eprintln!("{error}");continue;} },
                        Event::Paste(text)=>m::Input::Paste{text},
                        _=>continue,
                    };
                    if pending.pending_count()>=64 { eprintln!("Input queue full; input was not sent.");continue; }
                    let active=!matches!(&input,m::Input::Key{event} if event.action==m::KeyActionV2::Release);
                    let hints=m::InputHints{user_initiated:true,activity_hint:active,settled_at:settled.clone()};
                    match recovery.admit_input(input,hints) {Ok(input)=>pending.push(input),Err(error)=>eprintln!("Input not admitted: {error:?}")}
                }
                if !input_active { if let Some(input)=pending.pop() {let view=recovery.clone();input_job=Box::pin(async move{view.execute_input(input).await});input_active=true;} }
                if !resize_active {if let Some((cols,rows))=resize.take() {if cols>0&&rows>0 {match recovery.admit_resize(cols,rows) {Ok(input)=>{let view=recovery.clone();resize_job=Box::pin(async move{view.execute_resize(input).await});resize_active=true;},Err(error)=>eprintln!("Resize not admitted: {error:?}")}}}}
            }
            result=&mut input_job, if input_active=>{
                input_active=false; input_job=idle();
                let failed=match result {Ok(m::Reply::Success(_))=>false,Ok(m::Reply::Failure(error))=>{eprintln!("Input rejected: {:?}; not replayed.",error.code);true},Err(error)=>{eprintln!("Input outcome unknown: {error:?}; not replayed.");true}};
                if failed{if !pending.cancel_unsent(){eprintln!("Unsent tail fenced by view retirement.");}return Ok(if connection.status()==shellfox_client::manager_v2::Status::Connected{End::Recover}else{End::Reconnect});}
            }
            result=&mut ack_job, if ack_active=>{
                ack_active=false; ack_job=idle();
                match result {
                    Ok(m::Reply::Success(_))=>{if end_after_pump.is_some_and(|epoch|ack_epoch>=epoch){return Ok(End::Exited);}},
                    Err(ManagerRecoveryError::Engine(shellfox_mobile::Status::Busy))=>{if end_after_pump.is_some_and(|epoch|ack_epoch>=epoch){return Ok(End::Exited);}},
                    Err(error)=>return failure(error),
                    Ok(m::Reply::Failure(error))=>bail!("Native ACK rejected: {:?}",error.code),
                }
            }
            result=&mut catalog, if catalog_active=>{
                catalog_active=false;catalog=Box::pin(std::future::pending());
                match result {Ok(snapshot)=>{settled=settlement(&snapshot,tab);if !snapshot.entries.iter().any(|entry|entry.id()==session_id){settled=None;}},Err(error)=>eprintln!("Catalog refresh failed: {error:#}")}
            }
            result=&mut resize_job, if resize_active=>{resize_active=false;resize_job=idle();if let Err(error)=result{eprintln!("Resize failed: {error:?}");}}
            result=&mut poll=>{
                let response=match result {Ok(value)=>reply(value)?,Err(error)=>return failure(error)};
                for event in response.events {
                    match event {
                        m::HostEvent::Terminal{event:m::Event::Layout{recovery_required:true,..}}=>return Ok(End::Recover),
                        m::HostEvent::Terminal{event:m::Event::SourceEnded{..}|m::Event::ProcessExited{..}}=>end_after_pump=Some(pump_epoch.checked_add(1).ok_or_else(||anyhow!("Native owner pump cycle exhausted."))?),
                        m::HostEvent::Changed{..}=>refresh=true,
                        m::HostEvent::Terminal{event:m::Event::Revoked{reason:m::RevokedReason::ControlLost|m::RevokedReason::ControlReleased,..}}=>return Ok(End::Recover),
                        m::HostEvent::Terminal{event:m::Event::Revoked{reason,..}}=>bail!("Remote authority withdrawn: {reason:?}"),
                        m::HostEvent::Terminal{event:m::Event::Error{error,..}}=>bail!("Remote terminal error: {:?}",error.code),
                        _=>{},
                    }
                }
                context=ManagerPollContext::capture(&client,&[&recovery]).map_err(sdk)?;
                poll=Box::pin(context.poll(&client));
            }
        }
    }
}
