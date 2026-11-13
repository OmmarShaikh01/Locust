use crate::gui::theme::*;
use crate::ipc::framing::{read_frame, write_frame};
use crate::ipc::protocol::{ClientCommand, ModelSnapshot, ServerEvent};
use crate::ipc::socket::connect_stream;
use crate::logging::{self, Level};
use gpui::*;
use interprocess::TryClone;
use std::sync::mpsc::{self, Sender};
use std::thread;

mod cockpit;
mod dashboard;
mod kanban;
mod keymap;
mod servers;
mod shell;

pub struct CockpitApp {
    snapshot: ModelSnapshot,
    connected: bool,
    composer_input: String,
    composer_focused: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    draft_input: String,
    cmd_tx: Option<Sender<ClientCommand>>,
    focus_handle: FocusHandle,
    timeline_scroll_handle: ScrollHandle,
    sidebar_scroll_handle: ScrollHandle,
    matrix_scroll_handle: ScrollHandle,
    activity_scroll_handle: ScrollHandle,
    kanban_col0_scroll_handle: ScrollHandle,
    kanban_col1_scroll_handle: ScrollHandle,
    kanban_col2_scroll_handle: ScrollHandle,
    kanban_col3_scroll_handle: ScrollHandle,
    kanban_filter: usize,
    kanban_selected_item: Option<String>,
}

impl CockpitApp {
    pub fn new(pipe_name: String, cx: &mut Context<Self>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCommand>();
        let (event_tx, event_rx) = flume::unbounded::<ServerEvent>();

        let app = Self {
            snapshot: ModelSnapshot::default(),
            connected: false,
            composer_input: String::new(),
            composer_focused: true,
            history: Vec::new(),
            history_index: None,
            draft_input: String::new(),
            cmd_tx: Some(cmd_tx),
            focus_handle: cx.focus_handle(),
            timeline_scroll_handle: ScrollHandle::new(),
            sidebar_scroll_handle: ScrollHandle::new(),
            matrix_scroll_handle: ScrollHandle::new(),
            activity_scroll_handle: ScrollHandle::new(),
            kanban_col0_scroll_handle: ScrollHandle::new(),
            kanban_col1_scroll_handle: ScrollHandle::new(),
            kanban_col2_scroll_handle: ScrollHandle::new(),
            kanban_col3_scroll_handle: ScrollHandle::new(),
            kanban_filter: 0,
            kanban_selected_item: None,
        };

        cx.spawn(async move |entity, app| {
            while let Ok(event) = event_rx.recv_async().await {
                let _ = entity.update(app, |this: &mut CockpitApp, cx| match event {
                    ServerEvent::InitialSync {
                        state,
                        server_version: _,
                    } => {
                        this.snapshot = *state;
                        this.connected = true;
                        if this.snapshot.selected_tab == 2
                            && this.is_orchestrator_pane(this.snapshot.swarm_selected_pane)
                        {
                            this.composer_focused = true;
                        }
                        for msg in &this.snapshot.controller_history {
                            if (msg.role == "operator" || msg.role == "user")
                                && !msg.text.trim().is_empty()
                                && this.history.last().map(|s| s.as_str()) != Some(&msg.text)
                            {
                                this.history.push(msg.text.clone());
                            }
                        }
                        cx.notify();
                    }
                    ServerEvent::StateUpdated(state) => {
                        this.snapshot = *state;
                        cx.notify();
                    }
                    ServerEvent::TurnProgress {
                        pane_name,
                        delta_type,
                        content,
                    } => {
                        if let Some(pane) = this
                            .snapshot
                            .swarm_panes
                            .iter_mut()
                            .find(|p| p.name == pane_name)
                        {
                            if delta_type == "thinking" {
                                pane.thinking_lines.push(content);
                            } else {
                                pane.output_lines.push(content);
                            }
                            cx.notify();
                        }
                    }
                    _ => {}
                });
            }
        })
        .detach();

        thread::spawn(move || {
            let mut stream = match connect_stream(&pipe_name) {
                Ok(s) => s,
                Err(e) => {
                    logging::log(
                        Level::Error,
                        &format!("Failed to connect to daemon pipe: {e}"),
                    );
                    return;
                }
            };

            let mut write_stream = match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            };

            thread::spawn(move || {
                while let Ok(cmd) = cmd_rx.recv() {
                    if let Ok(bytes) = serde_json::to_vec(&cmd)
                        && write_frame(&mut write_stream, &bytes).is_err()
                    {
                        break;
                    }
                }
            });

            while let Ok(frame) = read_frame(&mut stream) {
                if let Ok(event) = serde_json::from_slice::<ServerEvent>(&frame)
                    && event_tx.send(event).is_err()
                {
                    break;
                }
            }
        });

        app
    }

    pub fn send_command(&self, cmd: ClientCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn is_orchestrator_pane(&self, pane_idx: usize) -> bool {
        if let Some(pane) = self.snapshot.swarm_panes.get(pane_idx) {
            let lower = pane.name.to_lowercase();
            if lower.contains("orchestrator") || lower.contains("controller") {
                return true;
            }
        }
        if pane_idx == 0
            && !self.snapshot.swarm_panes.iter().any(|p| {
                let l = p.name.to_lowercase();
                l.contains("orchestrator") || l.contains("controller")
            })
        {
            return true;
        }
        false
    }

    pub fn find_pane_by_name(&self, name: &str) -> Option<usize> {
        let name_lower = name.trim().to_lowercase();
        self.snapshot
            .swarm_panes
            .iter()
            .position(|p| p.name.to_lowercase() == name_lower)
    }

    pub fn orchestrator_pane_index(&self) -> usize {
        self.snapshot
            .swarm_panes
            .iter()
            .position(|p| {
                let l = p.name.to_lowercase();
                l.contains("orchestrator") || l.contains("controller")
            })
            .unwrap_or(0)
    }
}

impl Render for CockpitApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus_handle.is_focused(window) {
            window.focus(&self.focus_handle);
        }

        let perm_opt = self.snapshot.active_permission.clone();
        let selected_tab = self.snapshot.selected_tab;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(color_bg_base())
            .font_family(font_family_sans())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.handle_key_down(event, cx);
            }))
            .child(self.render_header(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .size_full()
                    .child(self.render_sidebar(cx))
                    .child(match selected_tab {
                        0 => self.render_dashboard(cx).into_any_element(),
                        1 => self.render_acp_servers(cx).into_any_element(),
                        2 => self.render_swarm_cockpit(cx).into_any_element(),
                        _ => self.render_kanban_tracker(cx).into_any_element(),
                    }),
            )
            .child(if let Some(perm) = &perm_opt {
                self.render_permission_modal(perm, cx).into_any_element()
            } else {
                div().into_any_element()
            })
    }
}

pub fn run_desktop(pipe_name: String) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let pipe_clone = pipe_name.clone();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Locust Cockpit".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| CockpitApp::new(pipe_clone, cx)),
        )
        .unwrap();
    });
}
