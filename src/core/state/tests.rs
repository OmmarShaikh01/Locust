use super::*;

#[test]
fn test_model_initialization() {
    let model = Model::default();
    assert_eq!(model.selected_tab, 0);
    assert_eq!(model.dashboard_frame, 0);
    assert_eq!(model.acp_selected_server, 0);
    assert!(!model.acp_servers.is_empty());
}

#[test]
fn test_dashboard_frame_cycling() {
    let mut model = Model::default();
    assert_eq!(model.dashboard_frame, 0);

    model.next_dashboard_frame();
    assert_eq!(model.dashboard_frame, 1);

    model.next_dashboard_frame();
    assert_eq!(model.dashboard_frame, 2);

    model.next_dashboard_frame();
    assert_eq!(model.dashboard_frame, 0);

    model.prev_dashboard_frame();
    assert_eq!(model.dashboard_frame, 2);
}

#[test]
fn test_tab_switching() {
    let mut model = Model::default();
    assert_eq!(model.selected_tab, 0);

    model.next_tab();
    assert_eq!(model.selected_tab, 1);

    model.next_tab();
    assert_eq!(model.selected_tab, 2);

    model.next_tab();
    assert_eq!(model.selected_tab, 3);

    model.next_tab();
    assert_eq!(model.selected_tab, 0);

    model.prev_tab();
    assert_eq!(model.selected_tab, 3);
}

#[test]
fn test_server_selection_wraps() {
    let mut model = Model::default();
    model.select_tab(1);
    let len = model.acp_servers.len();

    model.prev_server();
    assert_eq!(model.acp_selected_server, len - 1);

    model.next_server();
    assert_eq!(model.acp_selected_server, 0);
}

#[test]
fn test_swarm_panes_loaded_from_real_config() {
    let model = Model::default();
    assert!(
        model.swarm_config_error.is_none(),
        "{:?}",
        model.swarm_config_error
    );
    // 5 tier tabs (CTL 4 + SPC 3 + IMP 5 + VER 5 + OPS 4 named sub-panes) = 21 total.
    assert_eq!(model.swarm_panes.len(), 21);
}

#[test]
fn test_swarm_next_pane_advances_and_wraps() {
    let mut model = Model::default();
    model.select_tab(2);
    let len = model.swarm_panes.len();

    model.next_swarm_pane();
    assert_eq!(model.swarm_selected_pane, 1 % len);

    model.prev_swarm_pane();
    assert_eq!(model.swarm_selected_pane, 0);

    model.prev_swarm_pane();
    assert_eq!(model.swarm_selected_pane, len - 1);
}

#[test]
fn test_swarm_focus_toggles_and_resets_on_tab_switch() {
    let mut model = Model::default();
    model.select_tab(2);
    assert_eq!(model.swarm_focus, SwarmFocus::List);
    assert!(!model.is_controller_focused());

    model.toggle_swarm_focus();
    assert_eq!(model.swarm_focus, SwarmFocus::Controller);
    assert!(model.is_controller_focused());

    // Leaving and returning to the Swarm tab resets focus to the agent list.
    model.select_tab(0);
    model.select_tab(2);
    assert_eq!(model.swarm_focus, SwarmFocus::List);
}

#[test]
fn test_controller_type_backspace_and_submit() {
    let mut model = Model::default();
    model.controller_type_char('h');
    model.controller_type_char('i');
    assert_eq!(model.controller_input, "hi");

    model.controller_backspace();
    assert_eq!(model.controller_input, "h");

    model.controller_type_char('i');
    let sent = model.controller_submit();
    assert_eq!(sent, Some("hi".to_string()));
    assert_eq!(model.controller_input, "");
    assert_eq!(model.controller_log.len(), 1);
    assert_eq!(model.controller_log[0].role, ControllerRole::User);
    assert_eq!(model.controller_log[0].text, "hi");

    // Submitting empty/whitespace-only input sends nothing and logs nothing.
    model.controller_type_char(' ');
    assert_eq!(model.controller_submit(), None);
    assert_eq!(model.controller_log.len(), 1);
}

#[test]
fn test_controller_connection_status_mirrors_the_shared_acp_pool() {
    let mut model = Model::default();
    // Nobody has connected "acp-claude" yet.
    assert_eq!(model.controller_connection_status(), AcpStatus::Offline);

    // The Controller pane, the ACP Servers tab, and every Swarm pane run for this agent
    // all fold into the exact same acp_servers record -- there is only one status.
    model.apply_acp_update(AgentUpdate {
        server_id: CONTROLLER_SPEC_ID,
        outcome: AgentOutcome::Connected(crate::acp::HandshakeInfo {
            protocol_version: 1,
            agent_name: Some("Claude Code".to_string()),
            agent_version: None,
            capabilities: vec![],
            auth_methods: vec![],
            ping_ms: 10,
        }),
    });
    assert_eq!(model.controller_connection_status(), AcpStatus::Online);

    model.apply_acp_update(AgentUpdate {
        server_id: CONTROLLER_SPEC_ID,
        outcome: AgentOutcome::Failed(crate::acp::AcpError::Timeout),
    });
    assert_eq!(model.controller_connection_status(), AcpStatus::Error);
}

#[test]
fn test_push_controller_reply_logs_agent_or_system_role() {
    let mut model = Model::default();

    model.push_controller_reply(Ok("hi there".to_string()));
    assert_eq!(model.controller_log.len(), 1);
    assert_eq!(model.controller_log[0].role, ControllerRole::Agent);
    assert_eq!(model.controller_log[0].text, "hi there");

    model.push_controller_reply(Err("boom".to_string()));
    assert_eq!(model.controller_log.len(), 2);
    assert_eq!(model.controller_log[1].role, ControllerRole::System);
}

#[test]
fn test_directive_to_known_pane_is_approved_and_counted() {
    let mut model = Model::default();
    let target = model.swarm_panes[1].name.clone();
    let source = model.swarm_panes[0].name.clone();
    let text = format!("Done here.\n@[{target}]: please pick this up next");

    let directives = model.apply_swarm_run_outcome(RunOutcome::new(source, Ok(text)));

    assert_eq!(
        directives,
        vec![(target, "please pick this up next".to_string())]
    );
    assert_eq!(model.auto_routed_turns, 1);
}

#[test]
fn test_directive_to_unknown_pane_is_rejected_and_logged() {
    let mut model = Model::default();
    let source = model.swarm_panes[0].name.clone();
    let text = "@[Nonexistent Pane]: do the thing".to_string();

    let directives = model.apply_swarm_run_outcome(RunOutcome::new(source, Ok(text)));

    assert!(directives.is_empty());
    assert_eq!(model.auto_routed_turns, 0);
    assert!(
        model
            .activity_log
            .iter()
            .any(|l| l.contains("unknown agent") && l.contains("Nonexistent Pane"))
    );
}

#[test]
fn test_auto_route_budget_caps_total_dispatches() {
    let mut model = Model::default();
    let target = model.swarm_panes[1].name.clone();
    let source = model.swarm_panes[0].name.clone();
    let cap = model.limits.turn_cap;
    model.auto_routed_turns = cap;

    let directives = model.apply_swarm_run_outcome(RunOutcome::new(
        source,
        Ok(format!("@[{target}]: one more hop")),
    ));

    assert!(
        directives.is_empty(),
        "budget-exhausted directive must not be dispatched"
    );
    assert_eq!(model.auto_routed_turns, cap);
    assert!(
        model
            .activity_log
            .iter()
            .any(|l| l.contains("budget exhausted"))
    );
}

#[test]
fn test_directive_creates_pending_task_and_completion_marks_it_done() {
    let mut model = Model::default();
    let target = model.swarm_panes[1].name.clone();
    let source = model.swarm_panes[0].name.clone();

    model.apply_swarm_run_outcome(RunOutcome::new(
        source.clone(),
        Ok(format!("@[{target}]: please pick this up")),
    ));

    assert_eq!(model.task_counts(), (1, 0, 0));
    assert_eq!(model.tasks[0].source, source);
    assert_eq!(model.tasks[0].target, target);
    assert_eq!(model.tasks[0].status, TaskStatus::Pending);

    // The target pane's own turn finishing (successfully) is what completes the task --
    // not anything the agent says.
    model.apply_swarm_run_outcome(RunOutcome::new(target, Ok("done, thanks".to_string())));
    assert_eq!(model.task_counts(), (0, 1, 0));
}

#[test]
fn test_turn_events_update_live_state_while_running() {
    use crate::acp::TurnEvent;
    let mut model = Model::default();
    let name = model.swarm_panes[0].name.clone();
    model.begin_run_pane(0);
    assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Waiting);

    model.apply_turn_event(&name, TurnEvent::Thought("plan A\n".into()));
    assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Thinking);
    assert_eq!(model.swarm_panes[0].live.thought, "plan A\n");

    model.apply_turn_event(&name, TurnEvent::ToolStarted("Read file".into()));
    assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Tool);
    assert_eq!(model.swarm_panes[0].live.tool.as_deref(), Some("Read file"));

    model.apply_turn_event(
        &name,
        TurnEvent::ToolUpdate {
            title: None,
            done: true,
        },
    );
    assert_eq!(model.swarm_panes[0].live.tools_done, 1);
    assert!(model.swarm_panes[0].live.tool.is_none());

    model.apply_turn_event(&name, TurnEvent::Message("hel".into()));
    model.apply_turn_event(&name, TurnEvent::Message("lo".into()));
    assert_eq!(model.swarm_panes[0].live.phase, TurnPhase::Writing);
    assert_eq!(model.swarm_panes[0].output, "hello");
    assert_eq!(model.swarm_panes[0].status, SwarmRunStatus::Running);
}

#[test]
fn test_finished_turn_keeps_thinking_and_goes_idle() {
    use crate::acp::TurnEvent;
    let mut model = Model::default();
    let name = model.swarm_panes[0].name.clone();
    model.begin_run_pane(0);
    model.apply_turn_event(&name, TurnEvent::Thought("deep thought".into()));
    model.apply_swarm_run_outcome(RunOutcome::new(name.clone(), Ok("final".into())));
    let pane = &model.swarm_panes[0];
    assert_eq!(pane.status, SwarmRunStatus::Done);
    assert_eq!(pane.live.phase, TurnPhase::Idle);
    assert_eq!(pane.live.thought, "deep thought");
    assert_eq!(pane.output, "final");
    // Late events after the turn ended must not flip anything back.
    model.apply_turn_event(&name, TurnEvent::Message("late".into()));
    assert_eq!(model.swarm_panes[0].output, "final");
}

#[test]
fn test_thinking_text_is_capped() {
    use crate::acp::TurnEvent;
    let mut model = Model::default();
    let name = model.swarm_panes[0].name.clone();
    model.begin_run_pane(0);
    model.apply_turn_event(&name, TurnEvent::Thought("x".repeat(MAX_THOUGHT_CHARS)));
    model.apply_turn_event(&name, TurnEvent::Thought("END".into()));
    let t = &model.swarm_panes[0].live.thought;
    assert_eq!(t.chars().count(), MAX_THOUGHT_CHARS);
    assert!(t.ends_with("END"));
}

#[test]
fn test_permission_detail_is_redacted() {
    let mut model = Model::default();
    let (tx, _rx) = std::sync::mpsc::channel();
    model.set_permission_modal(crate::acp::manager::PermissionPrompt {
        id: 1,
        agent_pane: "A".into(),
        tool_title: "run".into(),
        detail: "curl -H 'Authorization: Bearer abc.def.ghi123' https://x".into(),
        expires_at: std::time::Instant::now() + std::time::Duration::from_secs(30),
        reply_to: tx,
    });
    let detail = &model.active_permission.as_ref().unwrap().detail;
    assert!(detail.contains("curl"));
    assert!(!detail.contains("abc.def.ghi123"), "{detail}");
}

#[test]
fn test_controller_scroll_moves_and_resets_on_submit() {
    let mut model = Model::default();
    model.controller_scroll_up(10);
    model.controller_scroll_up(5);
    assert_eq!(model.controller_scroll_back, 15);
    model.controller_scroll_down(100);
    assert_eq!(model.controller_scroll_back, 0);
    model.controller_scroll_top();
    model.controller_scroll_up(1); // must not overflow
    assert_eq!(model.controller_scroll_back, usize::MAX);
    model.controller_input = "hello".to_string();
    model.controller_submit();
    assert_eq!(model.controller_scroll_back, 0);
}

#[test]
fn test_scroll_back_moves_and_resets_on_new_turn() {
    let mut model = Model::default();
    model.scroll_pane_up(10);
    model.scroll_pane_up(5);
    assert_eq!(model.swarm_panes[0].scroll_back, 15);
    model.scroll_pane_down(20);
    assert_eq!(model.swarm_panes[0].scroll_back, 0);
    model.scroll_pane_top();
    assert_eq!(model.swarm_panes[0].scroll_back, usize::MAX);
    model.scroll_pane_up(1); // must not overflow
    model.begin_run_pane(0);
    assert_eq!(model.swarm_panes[0].scroll_back, 0);
}

fn two_panes(model: &Model) -> (String, String) {
    (
        model.swarm_panes[0].name.clone(),
        model.swarm_panes[1].name.clone(),
    )
}

#[test]
fn test_same_message_twice_is_sent_once() {
    let mut model = Model::default();
    let (a, b) = two_panes(&model);
    let text = format!("@[{b}]: review the diff");
    let first = model.apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())));
    let second = model.apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)));
    assert_eq!(first.len(), 1);
    assert!(second.is_empty(), "duplicate must be suppressed");
    assert_eq!(
        model.auto_routed_turns, 1,
        "duplicate must not use the turn cap"
    );
    assert_eq!(
        model.task_counts(),
        (1, 0, 0),
        "duplicate must not add a task"
    );
    assert!(
        model
            .activity_log
            .iter()
            .any(|l| l.contains("duplicate message"))
    );
}

#[test]
fn test_different_text_or_target_is_not_a_duplicate() {
    let mut model = Model::default();
    let (a, b) = two_panes(&model);
    let c = model.swarm_panes[2].name.clone();
    let out = model.apply_swarm_run_outcome(RunOutcome::new(
        a,
        Ok(format!("@[{b}]: one\n@[{b}]: two\n@[{c}]: one")),
    ));
    assert_eq!(out.len(), 3);
}

#[test]
fn test_failed_message_can_be_sent_again() {
    let mut model = Model::default();
    let (a, b) = two_panes(&model);
    let text = format!("@[{b}]: do the thing");
    model.apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())));
    model.apply_swarm_run_outcome(RunOutcome::new(b, Err("agent crashed".into())));
    let again = model.apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)));
    assert_eq!(again.len(), 1, "a failed message must be retryable");
}

#[test]
fn test_message_ids_survive_restart_and_expire() {
    let dir = std::env::temp_dir().join(format!("locust-idem-{}", std::process::id()));
    let path = dir.join("messages.json");
    let _ = std::fs::remove_dir_all(&dir);

    let mut model = Model::default();
    model.enable_message_store(path.clone());
    let (a, b) = two_panes(&model);
    let text = format!("@[{b}]: ship it");
    assert_eq!(
        model
            .apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())))
            .len(),
        1
    );

    // A new process (new Model) loading the same store must not resend.
    let mut restarted = Model::default();
    restarted.enable_message_store(path.clone());
    assert!(
        restarted
            .apply_swarm_run_outcome(RunOutcome::new(a.clone(), Ok(text.clone())))
            .is_empty()
    );

    // An entry older than the window is dropped on load, so the message may be sent again.
    let id = Model::message_id(&a, &b, "ship it");
    let old = std::collections::HashMap::from([(format!("{id:016x}"), 1u64)]);
    std::fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();
    let mut later = Model::default();
    later.enable_message_store(path);
    assert_eq!(
        later
            .apply_swarm_run_outcome(RunOutcome::new(a, Ok(text)))
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_project_modal_navigation() {
    let mut model = Model::default();
    model.open_project_modal("C:/cur".into(), vec!["C:/a".into(), "C:/b".into()]);
    model.project_modal_move(true);
    assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/a");
    model.project_modal_move(true);
    model.project_modal_move(true); // clamps at the last entry
    assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/b");
    model.project_modal_type('x');
    assert_eq!(model.project_modal.as_ref().unwrap().selected, None);
    model.project_modal_backspace();
    assert_eq!(model.project_modal.as_ref().unwrap().input, "C:/b");
}

#[test]
fn test_failed_turn_marks_dispatched_task_failed() {
    let mut model = Model::default();
    let target = model.swarm_panes[1].name.clone();
    let source = model.swarm_panes[0].name.clone();

    model.apply_swarm_run_outcome(RunOutcome::new(
        source,
        Ok(format!("@[{target}]: please pick this up")),
    ));
    model.apply_swarm_run_outcome(RunOutcome::new(target, Err("agent crashed".to_string())));

    assert_eq!(model.task_counts(), (0, 0, 1));
}

#[test]
fn test_unaddressed_pane_run_does_not_create_or_complete_a_task() {
    let mut model = Model::default();
    let pane = model.swarm_panes[0].name.clone();

    model.apply_swarm_run_outcome(RunOutcome::new(
        pane,
        Ok("just a normal startup run, no directive".to_string()),
    ));

    assert!(model.tasks.is_empty());
}

#[test]
fn test_controller_reply_directive_is_approved_too() {
    let mut model = Model::default();
    let target = model.swarm_panes[0].name.clone();

    let directives = model.push_controller_reply(Ok(format!("@[{target}]: start on this")));

    assert_eq!(directives, vec![(target, "start on this".to_string())]);
    assert_eq!(model.auto_routed_turns, 1);
}

#[test]
fn test_begin_run_selected_swarm_pane_marks_running() {
    let mut model = Model::default();
    model.select_tab(2);

    let assignment = model
        .begin_run_selected_swarm_pane()
        .expect("a pane is selected");
    assert_eq!(
        model.selected_swarm_pane().unwrap().status,
        SwarmRunStatus::Running
    );
    assert_eq!(assignment.pane, model.selected_swarm_pane().unwrap().name);
}

#[test]
fn test_apply_swarm_run_outcome_updates_matching_pane() {
    let mut model = Model::default();
    let pane_name = model.swarm_panes[0].name.clone();
    model.swarm_panes[0].status = SwarmRunStatus::Running;

    model.apply_swarm_run_outcome(crate::swarm::RunOutcome::new(
        pane_name.clone(),
        Ok("done talking".to_string()),
    ));

    let pane = model
        .swarm_panes
        .iter()
        .find(|p| p.name == pane_name)
        .unwrap();
    assert_eq!(pane.status, SwarmRunStatus::Done);
    assert_eq!(pane.output, "done talking");
}

#[test]
fn test_apply_swarm_run_outcome_records_usage_telemetry() {
    let mut model = Model::default();
    let pane_name = model.swarm_panes[0].name.clone();

    let usage = crate::acp::UsageInfo {
        tokens_used: Some(1500),
        context_size: Some(128000),
        cost_amount: Some(0.02),
        cost_currency: Some("USD".to_string()),
        stop_reason: Some("end_turn".to_string()),
    };

    model.apply_swarm_run_outcome(
        RunOutcome::new(pane_name.clone(), Ok("all done".to_string())).with_usage(Some(usage)),
    );

    let pane = model
        .swarm_panes
        .iter()
        .find(|p| p.name == pane_name)
        .unwrap();
    assert_eq!(pane.tokens_used, Some(1500));
    assert_eq!(pane.context_size, Some(128000));
    assert_eq!(pane.tokens_left(), Some(126500));
    assert_eq!(
        pane.percent_left().map(|p| (p * 10.0).round() / 10.0),
        Some(98.8)
    );
    assert_eq!(pane.cost, Some((0.02, "USD".to_string())));
    assert!(model.acp_tokens_used() >= 1500);
    assert_eq!(model.acp_tokens_left(), Some(126500));
}

#[test]
fn test_acp_server_tokens_left_and_percentage() {
    let mut server = AcpServer::from_spec(&crate::acp::AGENT_SPECS[0]);
    assert_eq!(server.tokens_left(), None);
    assert_eq!(server.percent_left(), None);

    server.tokens_used = Some(5000);
    server.context_size = Some(200_000);
    assert_eq!(server.tokens_left(), Some(195_000));
    assert_eq!(server.percent_left(), Some(97.5));
}

#[test]
fn test_broadcast_to_all_panes() {
    let mut model = Model::default();
    let source = model.swarm_panes[0].name.clone();
    let directives = model.apply_swarm_run_outcome(RunOutcome::new(
        source.clone(),
        Ok("@[*]: alert everyone".to_string()),
    ));

    // Must dispatch to all panes except source
    assert_eq!(directives.len(), model.swarm_panes.len() - 1);
    assert!(!directives.iter().any(|(target, _)| target == &source));
    for (target, msg) in directives {
        assert_eq!(msg, "alert everyone");
        assert!(model.swarm_panes.iter().any(|p| p.name == target));
    }
}

#[test]
fn test_broadcast_to_tab() {
    let mut model = Model::default();
    let tab = model.swarm_panes[0].tab.clone();
    let tab_pane_count = model.swarm_panes.iter().filter(|p| p.tab == tab).count();

    let directives = model.push_controller_reply(Ok(format!("@[{tab}]: tab notice")));
    assert_eq!(directives.len(), tab_pane_count);
    for (target, msg) in directives {
        assert_eq!(msg, "tab notice");
        assert!(
            model
                .swarm_panes
                .iter()
                .any(|p| p.name == target && p.tab == tab)
        );
    }
}

#[test]
fn test_reply_loop_between_two_agents_stops_at_turn_cap() {
    let mut model = Model::default();
    model.limits.turn_cap = 10;
    let agent_a = model.swarm_panes[0].name.clone();
    let agent_b = model.swarm_panes[1].name.clone();

    let mut current_speaker = agent_a.clone();
    let mut loop_count = 0;

    loop {
        let next_target = if current_speaker == agent_a {
            &agent_b
        } else {
            &agent_a
        };
        // Each message differs, so only the turn cap can stop this loop.
        let text = format!("@[{next_target}]: ping pong {loop_count}");
        let directives =
            model.apply_swarm_run_outcome(RunOutcome::new(current_speaker.clone(), Ok(text)));
        if directives.is_empty() {
            break;
        }
        loop_count += 1;
        current_speaker = next_target.clone();
        assert!(loop_count <= 20, "loop runaway guard");
    }

    assert_eq!(model.auto_routed_turns, 10);
    assert_eq!(loop_count, 10);
}

#[test]
fn test_identical_ping_pong_is_stopped_by_idempotency() {
    let mut model = Model::default();
    let a = model.swarm_panes[0].name.clone();
    let b = model.swarm_panes[1].name.clone();
    let mut speaker = a.clone();
    let mut turns = 0;
    loop {
        let target = if speaker == a { &b } else { &a };
        let text = format!("@[{target}]: ping pong");
        let out = model.apply_swarm_run_outcome(RunOutcome::new(speaker.clone(), Ok(text)));
        if out.is_empty() {
            break;
        }
        turns += 1;
        speaker = target.clone();
        assert!(turns < 10, "must stop on its own");
    }
    // a->b and b->a are different ids, then a->b repeats and is suppressed.
    assert_eq!(turns, 2);
}

#[test]
fn test_token_limit_blocks_new_directives() {
    let mut model = Model::default();
    model.limits.token_limit = Some(1000);
    model.telemetry.total_tokens = 1500;
    let target = model.swarm_panes[1].name.clone();
    let source = model.swarm_panes[0].name.clone();

    let directives =
        model.apply_swarm_run_outcome(RunOutcome::new(source, Ok(format!("@[{target}]: do work"))));

    assert!(directives.is_empty());
    assert!(
        model
            .activity_log
            .iter()
            .any(|l| l.contains("token limit (1000) reached"))
    );
}

#[test]
fn test_export_transcripts_and_redacts_secrets() {
    let mut model = Model::default();
    let pane_name = model.swarm_panes[0].name.clone();
    model.swarm_panes[0].output =
        "Sensitive key: AKIA1234567890ABCDEF, Bearer secrettoken123".to_string();
    model.controller_log.push(ControllerMessage {
        role: ControllerRole::User,
        text: "User secret: xoxb-1234567890-abcdef".to_string(),
    });

    let temp_dir = std::env::temp_dir().join(format!("locust_test_export_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let export_dir = model
        .export_transcripts(Some(&temp_dir))
        .expect("export should succeed");
    assert!(export_dir.exists());

    // Check pane markdown
    let pane_file = export_dir.join(format!("{}.md", sanitize_filename(&pane_name)));
    assert!(pane_file.exists());
    let pane_content = std::fs::read_to_string(pane_file).unwrap();
    assert!(!pane_content.contains("AKIA1234567890ABCDEF"));
    assert!(!pane_content.contains("secrettoken123"));
    assert!(pane_content.contains("[REDACTED"));

    // Check Controller markdown
    let ctl_file = export_dir.join("Controller.md");
    assert!(ctl_file.exists());
    let ctl_content = std::fs::read_to_string(ctl_file).unwrap();
    assert!(!ctl_content.contains("xoxb-1234567890-abcdef"));
    assert!(ctl_content.contains("[REDACTED"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cancel_running_pane() {
    let mut model = Model::default();
    let target = model.swarm_panes[0].name.clone();
    model.swarm_panes[0].status = SwarmRunStatus::Running;
    model.create_task("Controller", &target, "some task");

    let cancelled = model.cancel_swarm_pane(0);
    assert_eq!(cancelled, Some(target.clone()));
    assert_eq!(model.swarm_panes[0].status, SwarmRunStatus::Error);
    assert_eq!(model.task_counts(), (0, 0, 1));
    assert!(
        model
            .activity_log
            .iter()
            .any(|l| l.contains("cancelled by operator"))
    );
}

#[test]
fn test_permission_modal_timeout() {
    let mut model = Model::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let prompt = crate::acp::manager::PermissionPrompt {
        id: 100,
        agent_pane: "Test Agent".to_string(),
        tool_title: "dangerous_tool".to_string(),
        detail: String::new(),
        expires_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
        reply_to: tx,
    };
    model.set_permission_modal(prompt);
    assert!(model.active_permission.is_some());

    assert!(model.check_permission_timeout());
    assert!(model.active_permission.is_none());

    let decision = rx.recv().expect("must send deny decision on timeout");
    assert_eq!(decision, crate::acp::client::PermissionDecision::Deny);
}
