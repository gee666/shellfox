use super::*;

impl TerminalView {
    pub(in crate::app) fn poll_peer(&mut self, cx: &mut Context<Self>) {
        let Some(peer) = self.peer_mut() else {
            return;
        };
        let state = peer.binding.state();
        if state.status != ConnectionStatus::Connected {
            if !matches!(peer.state, State::Offline) {
                peer.state = State::Offline;
                peer.clear_operations();
                cx.notify();
            }
            return;
        }
        if state.connection_id != peer.connection {
            peer.connection = state.connection_id;
            peer.detach_failed = false;
            peer.state = State::Offline;
            peer.clear_operations();
            cx.notify();
        }
        if peer.in_flight {
            return;
        }
        let operation = if let Some(ack) = peer.ack.take() {
            Operation::Ack(ack)
        } else if let Some(write) = peer.writes.pop_front() {
            Operation::Write(write)
        } else if peer.frozen {
            return;
        } else if !peer.visible {
            if peer.detach_failed {
                return;
            }
            let Some(generation) = peer.attachment_generation else {
                return;
            };
            Operation::Detach(DetachTerminal {
                tab_id: peer.tab,
                generation,
            })
        } else {
            match &peer.state {
                State::Offline => {
                    peer.state = State::Attaching(Vec::new());
                    Operation::Attach {
                        tab_id: peer.tab,
                        takeover: None,
                    }
                }
                State::Attached { cursor, open: true } => {
                    let Some((cols, rows)) = peer.size_to_send.take() else {
                        return;
                    };
                    let Ok(resize) = cursor.resize(cols, rows, peer.connection) else {
                        return;
                    };
                    if let Err(error) = peer
                        .terminal
                        .resize(cols, rows)
                        .and_then(|()| peer.terminal.drain_replies())
                    {
                        peer.unavailable(error.to_string());
                        cx.notify();
                        return;
                    }
                    self.refresh_peer_screen();
                    Operation::Resize(resize)
                }
                _ => return,
            }
        };
        self.run_peer_operation(operation, cx);
    }

    fn run_peer_operation(&mut self, operation: Operation, cx: &mut Context<Self>) {
        let Some(peer) = self.peer_mut() else {
            return;
        };
        let binding = peer.binding.clone();
        let connection = peer.connection;
        peer.in_flight = true;
        let detaching = matches!(&operation, Operation::Detach(_));
        cx.spawn(async move |view, cx| {
            let result = operation.run(&binding).await;
            let _ = view.update(cx, |view, cx| {
                let colors = view.theme.colors();
                let Some(peer) = view.peer_mut() else {
                    return;
                };
                peer.in_flight = false;
                let state = peer.binding.state();
                if state.connection_id == connection && state.status == ConnectionStatus::Connected
                {
                    match result {
                        Ok(Reply::Success(Applied::Attached(attachment))) => {
                            if let Err(error) = peer.attach(attachment, &colors) {
                                peer.unavailable(error.to_string());
                            }
                            view.refresh_peer_screen();
                        }
                        Ok(Reply::Success(Applied::Detached)) => {
                            peer.state = State::Offline;
                            peer.attachment_generation = None;
                            peer.clear_operations();
                        }
                        Ok(Reply::Success(Applied::Other)) => {}
                        Ok(Reply::Failure(_)) | Err(_) if detaching => peer.detach_failed = true,
                        Ok(Reply::Failure(error)) => peer.operation_failed(error),
                        Err(error) => peer.unavailable(error.to_string()),
                    }
                }
                view.poll_peer(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn take_peer_control(&mut self, cx: &mut Context<Self>) {
        let Some(peer) = self.peer_mut() else {
            return;
        };
        if peer.in_flight
            || peer.frozen
            || peer.binding.state().status != ConnectionStatus::Connected
        {
            return;
        }
        if let State::Blocked(generation) = peer.state {
            peer.state = State::Attaching(Vec::new());
            let tab_id = peer.tab;
            self.run_peer_operation(
                Operation::Attach {
                    tab_id,
                    takeover: Some(generation),
                },
                cx,
            );
        }
    }
}
