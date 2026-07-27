use std::time::{Duration, Instant};

use super::{
    background_update_check_enabled, App, AUTO_UPDATE_CHECK_INTERVAL, MIN_RENDER_INTERVAL,
};
fn retain_detached_process_after_wait(
    pid: u32,
    result: std::io::Result<Option<std::process::ExitStatus>>,
) -> bool {
    match result {
        Ok(None) => true,
        Ok(Some(_)) => false,
        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => true,
        Err(err) => {
            tracing::warn!(pid, err = %err, "failed to reap detached process");
            false
        }
    }
}

/// How long a pane waits for the host terminal to answer a forwarded OSC 52
/// clipboard query before receiving an empty reply. Generous because some
/// terminals (e.g. kitty) ask the user for permission on first read.
pub(crate) const HOST_CLIPBOARD_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound on panes concurrently waiting for a host clipboard reply.
/// Queries past the cap get an immediate empty reply instead of queueing.
const MAX_PENDING_HOST_CLIPBOARD_QUERIES: usize = 16;

impl App {
    /// Records a pane waiting on a host clipboard reply. Returns false when
    /// the pending queue is full; the caller must then answer the pane with
    /// an empty reply instead of forwarding the query.
    pub(crate) fn register_host_clipboard_query(&mut self, pane_id: crate::layout::PaneId) -> bool {
        if self.pending_host_clipboard_queries.len() >= MAX_PENDING_HOST_CLIPBOARD_QUERIES {
            return false;
        }
        self.pending_host_clipboard_queries
            .push_back((pane_id, Instant::now() + HOST_CLIPBOARD_REPLY_TIMEOUT));
        true
    }

    /// Routes a host terminal's OSC 52 reply to the oldest pane still waiting
    /// on one. Replies with no pending query are dropped: herdr never wrote a
    /// query, so nothing may be pasted into a pane from them.
    pub(crate) fn resolve_host_clipboard_reply(&mut self, data: &str) {
        let Some((pane_id, _deadline)) = self.pending_host_clipboard_queries.pop_front() else {
            tracing::debug!("dropping host OSC 52 clipboard reply with no pending query");
            return;
        };
        self.answer_pane_clipboard_query(pane_id, data);
    }

    /// Writes an OSC 52 paste reply built from `data` (base64, possibly
    /// empty) into the pane's PTY.
    pub(crate) fn answer_pane_clipboard_query(
        &mut self,
        pane_id: crate::layout::PaneId,
        data: &str,
    ) {
        let reply = crate::pane::osc52_paste_reply_from_base64(data);
        let Some(runtime) = self
            .state
            .runtime_for_pane(&self.terminal_runtimes, pane_id)
        else {
            tracing::debug!(
                pane = pane_id.raw(),
                "pane waiting on OSC 52 clipboard reply no longer exists"
            );
            return;
        };
        if let Err(err) = runtime.try_send_bytes(reply) {
            tracing::warn!(
                pane = pane_id.raw(),
                err = %err,
                "failed to write OSC 52 clipboard reply to pane"
            );
        }
    }

    /// Answers panes whose host clipboard reply deadline passed with an empty
    /// reply so the waiting application unblocks.
    pub(crate) fn expire_host_clipboard_queries(&mut self, now: Instant) {
        while self
            .pending_host_clipboard_queries
            .front()
            .is_some_and(|(_, deadline)| *deadline <= now)
        {
            if let Some((pane_id, _deadline)) = self.pending_host_clipboard_queries.pop_front() {
                tracing::debug!(
                    pane = pane_id.raw(),
                    "host OSC 52 clipboard reply timed out; sending empty reply"
                );
                self.answer_pane_clipboard_query(pane_id, "");
            }
        }
    }

    pub(crate) fn reap_finished_detached_processes(&mut self) {
        self.detached_process_children
            .retain_mut(|child| retain_detached_process_after_wait(child.id(), child.try_wait()));
    }

    pub(crate) fn shutdown_terminal_runtime(&mut self, terminal_id: crate::terminal::TerminalId) {
        if let Some(runtime) = self.terminal_runtimes.remove(&terminal_id) {
            runtime.shutdown();
        }
    }

    pub(crate) fn shutdown_detached_terminal_runtimes(&mut self) {
        let terminal_ids = std::mem::take(&mut self.state.terminal_runtime_shutdowns);
        for terminal_id in terminal_ids {
            self.shutdown_terminal_runtime(terminal_id);
        }
    }

    pub(crate) fn sync_agent_metadata_deadline(&mut self) {
        self.agent_metadata_deadline = self.state.next_agent_metadata_expiry();
    }

    pub(crate) fn expire_due_metadata(&mut self, now: Instant) -> bool {
        let Some(deadline) = self
            .agent_metadata_deadline
            .filter(|deadline| now >= *deadline)
        else {
            return false;
        };
        self.expire_metadata_at(deadline, now);
        true
    }

    pub(crate) fn expire_metadata_at(&mut self, deadline: Instant, now: Instant) {
        let previous_toast = self.state.toast.clone();
        for update in self.state.expire_agent_metadata_at(deadline, now) {
            self.refresh_new_herdr_toast_context_for_update(&update, &previous_toast);
            self.emit_pane_state_update(&update);
        }
        let (panes, workspaces) = self.state.expire_metadata_tokens(now);
        for (ws_idx, pane_id) in panes {
            self.emit_pane_updated(ws_idx, pane_id);
        }
        for ws_idx in workspaces {
            self.emit_workspace_token_updated(ws_idx);
        }
        self.sync_agent_metadata_deadline();
    }

    pub(crate) fn can_render_now(&self, now: Instant) -> bool {
        match self.last_render_at {
            Some(last_render_at) => now.duration_since(last_render_at) >= MIN_RENDER_INTERVAL,
            None => true,
        }
    }

    pub(crate) fn can_present_now(&self, now: Instant) -> bool {
        match self.last_presentation_at {
            Some(last_presentation_at) => {
                now.duration_since(last_presentation_at) >= MIN_RENDER_INTERVAL
            }
            None => true,
        }
    }

    pub(crate) fn record_render_attempt(&mut self, now: Instant, presentation: bool) {
        self.last_render_at = Some(now);
        if presentation {
            self.last_presentation_at = Some(now);
        }
    }

    pub(crate) fn run_auto_update_check(&mut self) {
        if !background_update_check_enabled(
            self.policy.background_updates,
            self.update_version_check_enabled,
        ) {
            self.next_auto_update_check = None;
            return;
        }

        self.next_auto_update_check = self
            .state
            .update_available
            .is_none()
            .then_some(Instant::now() + AUTO_UPDATE_CHECK_INTERVAL);

        if self.state.update_available.is_some() {
            return;
        }

        let update_tx = self.event_tx.clone();
        std::thread::spawn(move || crate::update::auto_update(update_tx));
    }

    pub(crate) fn run_agent_manifest_update_check(&mut self) {
        if !background_update_check_enabled(
            self.policy.background_updates,
            self.update_manifest_check_enabled,
        ) {
            self.next_agent_manifest_update_check = None;
            return;
        }

        self.next_agent_manifest_update_check = Some(Instant::now() + AUTO_UPDATE_CHECK_INTERVAL);

        let manifest_update_tx = self.event_tx.clone();
        std::thread::spawn(move || crate::detect::manifest_update::auto_update(manifest_update_tx));
    }

    pub(crate) fn next_headless_loop_deadline_with_git_refresh(
        &self,
        now: Instant,
        needs_render: bool,
        include_git_refresh: bool,
    ) -> Option<Instant> {
        let render_deadline = if needs_render {
            self.last_render_at
                .map(|last_render_at| last_render_at + MIN_RENDER_INTERVAL)
                .filter(|deadline| *deadline > now)
        } else {
            None
        };

        [
            self.config_diagnostic_deadline,
            self.toast_deadline,
            self.state.next_pending_agent_notification_deadline(),
            self.state.next_managed_agent_deadline(),
            include_git_refresh
                .then(|| self.git_refresh_deadline())
                .flatten(),
            self.next_auto_update_check,
            self.next_agent_manifest_update_check,
            self.agent_metadata_deadline,
            self.pending_agent_resume_deadline,
            self.session_save_deadline,
            self.next_tab_bar_status_deadline(),
            render_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    #[cfg(test)]
    pub(crate) fn drain_internal_events(&mut self) -> bool {
        self.drain_internal_events_up_to(super::APP_EVENT_DRAIN_LIMIT)
            .1
    }

    #[cfg(test)]
    pub(crate) fn drain_all_internal_events(&mut self) -> bool {
        let mut changed = false;
        loop {
            let (had_event, batch_changed) =
                self.drain_internal_events_up_to(super::APP_EVENT_DRAIN_LIMIT);
            changed |= batch_changed;
            if !had_event {
                break;
            }
        }
        changed
    }

    #[cfg(test)]
    fn drain_internal_events_up_to(&mut self, limit: usize) -> (bool, bool) {
        let mut had_event = false;
        let mut changed = false;
        for _ in 0..limit {
            let Ok(ev) = self.event_rx.try_recv() else {
                break;
            };
            had_event = true;
            changed |= self.handle_internal_event_with_render_impact(ev);
        }
        (had_event, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn hidden_render_attempt_keeps_presentation_cadence_available() {
        let (mut app, _) = test_app_with_pane();
        let initial_presentation = Instant::now();
        app.record_render_attempt(initial_presentation, true);

        let hidden_attempt = initial_presentation + MIN_RENDER_INTERVAL;
        app.record_render_attempt(hidden_attempt, false);
        let foreground_echo = hidden_attempt + Duration::from_millis(1);

        assert!(!app.can_render_now(foreground_echo));
        assert!(app.can_present_now(foreground_echo));
    }

    #[test]
    fn interrupted_detached_process_wait_keeps_child_for_retry() {
        let interrupted = std::io::Error::new(std::io::ErrorKind::Interrupted, "test interrupt");

        assert!(retain_detached_process_after_wait(42, Err(interrupted)));
    }

    fn test_app_with_pane() -> (super::super::App, crate::layout::PaneId) {
        let mut app = super::super::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        );
        let ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        app.state.workspaces.push(ws);
        app.state.active = Some(0);
        app.state.view.pane_infos.push(crate::layout::PaneInfo {
            id: pane_id,
            rect: ratatui::layout::Rect::new(0, 0, 80, 24),
            inner_rect: ratatui::layout::Rect::new(0, 0, 80, 24),
            scrollbar_rect: None,
            borders: ratatui::widgets::Borders::NONE,
            is_focused: true,
        });
        (app, pane_id)
    }

    fn test_app_with_pane_channel() -> (
        super::super::App,
        crate::layout::PaneId,
        tokio::sync::mpsc::Receiver<bytes::Bytes>,
    ) {
        let (mut app, pane_id) = test_app_with_pane();
        let (runtime, receiver) = crate::terminal::TerminalRuntime::test_with_channel_capacity(
            80,
            24,
            MAX_PENDING_HOST_CLIPBOARD_QUERIES + 1,
        );
        app.state.workspaces[0].tabs[0]
            .runtimes
            .insert(pane_id, runtime);
        (app, pane_id, receiver)
    }

    #[tokio::test]
    async fn host_clipboard_reply_routes_to_oldest_pending_pane() {
        let (mut app, pane_id, mut receiver) = test_app_with_pane_channel();

        assert!(app.register_host_clipboard_query(pane_id));
        app.resolve_host_clipboard_reply("aGVsbG8=");

        assert_eq!(
            receiver.try_recv().unwrap(),
            bytes::Bytes::from_static(b"\x1b]52;c;aGVsbG8=\x07")
        );
        assert!(app.pending_host_clipboard_queries.is_empty());
    }

    #[tokio::test]
    async fn host_clipboard_reply_without_pending_query_writes_nothing() {
        let (mut app, _pane_id, mut receiver) = test_app_with_pane_channel();

        app.resolve_host_clipboard_reply("aGVsbG8=");

        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn invalid_host_clipboard_reply_payload_answers_empty() {
        let (mut app, pane_id, mut receiver) = test_app_with_pane_channel();

        assert!(app.register_host_clipboard_query(pane_id));
        app.resolve_host_clipboard_reply("not base64!");

        assert_eq!(
            receiver.try_recv().unwrap(),
            bytes::Bytes::from_static(b"\x1b]52;c;\x07")
        );
    }

    #[tokio::test]
    async fn expired_host_clipboard_queries_get_empty_replies() {
        let (mut app, pane_id, mut receiver) = test_app_with_pane_channel();

        assert!(app.register_host_clipboard_query(pane_id));
        app.expire_host_clipboard_queries(Instant::now());
        assert!(receiver.try_recv().is_err(), "deadline not reached yet");

        app.expire_host_clipboard_queries(
            Instant::now() + HOST_CLIPBOARD_REPLY_TIMEOUT + Duration::from_secs(1),
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            bytes::Bytes::from_static(b"\x1b]52;c;\x07")
        );
        assert!(app.pending_host_clipboard_queries.is_empty());
    }

    #[tokio::test]
    async fn host_clipboard_query_queue_is_bounded() {
        let (mut app, pane_id, _receiver) = test_app_with_pane_channel();

        for _ in 0..MAX_PENDING_HOST_CLIPBOARD_QUERIES {
            assert!(app.register_host_clipboard_query(pane_id));
        }
        assert!(!app.register_host_clipboard_query(pane_id));
    }
}
