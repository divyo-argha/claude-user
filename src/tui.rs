use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::io::{stdout, Stdout};

use crate::aliases;
use crate::mappings;
use crate::profiles;

type CuserTerminal = Terminal<CrosstermBackend<Stdout>>;

const NEW_PROFILE: &str = "+ Add new account / profile (press 'n')";
const IMPORT_DEFAULT: &str = "+ Import ~/.claude account (press 'i')";

pub enum PickResult {
    Existing(String),
    New(String),
    Import(String),
}

enum Action {
    New,
    Import,
    Rename { old: String },
    Alias { profile: String },
}

enum StatusMessage {
    Error(String),
    Info(String),
}

enum Mode {
    Picking,
    Naming { action: Action, buffer: String },
    ConfirmDelete { name: String },
}

enum Item {
    Profile {
        name: String,
        alias: Option<String>,
        email: Option<String>,
        org_name: Option<String>,
        is_current: bool,
        is_disabled: bool,
        is_mapped: bool,
        usage: Option<Box<crate::usage::AccountUsage>>,
    },
    ImportDefault,
    NewProfile,
}

pub fn run_picker() -> Result<Option<PickResult>> {
    let (items, current_profile, current_idx) = build_items()?;

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal: CuserTerminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, items, current_profile, current_idx);

    disable_raw_mode().ok();
    stdout().execute(LeaveAlternateScreen).ok();

    result
}

fn build_items() -> Result<(Vec<Item>, Option<String>, Option<usize>)> {
    let current = profiles::current_profile()?;
    let cwd = std::env::current_dir().ok();
    let mapped_profile = cwd.as_ref().and_then(|p| {
        mappings::resolve_mapping(p).ok().flatten().map(|(name, _)| name)
    });
    let aliases_map = aliases::load_aliases().unwrap_or_default();

    let mut items = Vec::new();
    let mut current_idx = None;

    for name in profiles::list_profiles()? {
        let is_current = current.as_deref() == Some(&name);
        let is_mapped = mapped_profile.as_deref() == Some(&name);
        let is_disabled = profiles::is_profile_disabled(&name).unwrap_or(false);

        if is_current {
            current_idx = Some(items.len());
        }
        let info = profiles::get_profile_info(&name).unwrap_or(profiles::ProfileInfo {
            name: name.clone(),
            email: None,
            org_name: None,
            is_disabled,
        });

        let my_aliases: Vec<&str> = aliases_map
            .iter()
            .filter(|(_, target)| target.as_str() == name)
            .map(|(a, _)| a.as_str())
            .collect();
        let primary_alias = my_aliases.first().map(|s| s.to_string());

        let usage_data = crate::usage::get_profile_usage(&name, false).ok().map(Box::new);
        items.push(Item::Profile {
            name,
            alias: primary_alias,
            email: info.email,
            org_name: info.org_name,
            is_current,
            is_disabled,
            is_mapped,
            usage: usage_data,
        });
    }

    if current_idx.is_none() && mapped_profile.is_some() {
        for (i, it) in items.iter().enumerate() {
            if let Item::Profile { is_mapped: true, .. } = it {
                current_idx = Some(i);
                break;
            }
        }
    }

    if profiles::can_import()? {
        items.push(Item::ImportDefault);
    }
    items.push(Item::NewProfile);
    Ok((items, current, current_idx))
}

fn selected_profile_name(items: &[Item], state: &ListState) -> Option<String> {
    match items.get(state.selected().unwrap_or(0)) {
        Some(Item::Profile { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

fn event_loop(
    terminal: &mut CuserTerminal,
    mut items: Vec<Item>,
    mut current_profile: Option<String>,
    current_idx: Option<usize>,
) -> Result<Option<PickResult>> {
    let mut list_state = ListState::default();
    list_state.select(Some(current_idx.unwrap_or(0)));
    let mut mode = Mode::Picking;
    let mut status: Option<StatusMessage> = None;

    loop {
        terminal.draw(|f| draw(f, &items, &mut list_state, &mode, &status, &current_profile))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(None);
        }

        match &mut mode {
            Mode::Picking => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('+') => {
                    mode = Mode::Naming {
                        action: Action::New,
                        buffer: String::new(),
                    };
                    status = None;
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    if profiles::can_import().unwrap_or(false) {
                        mode = Mode::Naming {
                            action: Action::Import,
                            buffer: String::new(),
                        };
                        status = None;
                    } else {
                        status = Some(StatusMessage::Info(
                            "No default ~/.claude directory available to import.".to_string(),
                        ));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                    let i = list_state.selected().unwrap_or(0);
                    if i == 0 {
                        list_state.select(Some(items.len().saturating_sub(1)));
                    } else {
                        list_state.select(Some(i - 1));
                    }
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    let i = list_state.selected().unwrap_or(0);
                    if i + 1 < items.len() {
                        list_state.select(Some(i + 1));
                    } else {
                        list_state.select(Some(0));
                    }
                }
                KeyCode::Home => {
                    list_state.select(Some(0));
                }
                KeyCode::End => {
                    list_state.select(Some(items.len().saturating_sub(1)));
                }
                KeyCode::PageUp => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(5)));
                }
                KeyCode::PageDown => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some((i + 5).min(items.len().saturating_sub(1))));
                }
                KeyCode::Char('s') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        match profiles::activate_profile(&name) {
                            Ok(()) => {
                                current_profile = Some(name.clone());
                                let sel = list_state.selected();
                                let (new_items, _, _) = build_items()?;
                                items = new_items;
                                list_state.select(sel);
                                status = Some(StatusMessage::Info(format!(
                                    "Switched active profile to \"{name}\". Run `claude` to use it."
                                )));
                            }
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        }
                    }
                }
                KeyCode::Char('a') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        let existing = aliases::aliases_for_profile(&name).unwrap_or_default();
                        let initial = existing.first().cloned().unwrap_or_default();
                        mode = Mode::Naming {
                            action: Action::Alias { profile: name },
                            buffer: initial,
                        };
                        status = None;
                    }
                }
                KeyCode::Char('m') => {
                    if let Some(name) = selected_profile_name(&items, &list_state)
                        && let Ok(cwd) = std::env::current_dir()
                    {
                        let is_mapped_to_selected = mappings::resolve_mapping(&cwd)
                            .ok()
                            .flatten()
                            .map(|(p, _)| p == name)
                            .unwrap_or(false);

                        let cwd_str = cwd.to_string_lossy();
                        if is_mapped_to_selected {
                            match mappings::remove_mapping(Some(cwd_str.as_ref())) {
                                Ok((path, Some(prev))) => {
                                    let sel = list_state.selected();
                                    let (new_items, _, _) = build_items()?;
                                    items = new_items;
                                    list_state.select(sel);
                                    status = Some(StatusMessage::Info(format!(
                                        "Unmapped directory \"{}\" from \"{prev}\".",
                                        path.display()
                                    )));
                                }
                                Ok((_, None)) => {}
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        } else {
                            match mappings::add_mapping(&name, Some(cwd_str.as_ref())) {
                                Ok(path) => {
                                    let sel = list_state.selected();
                                    let (new_items, _, _) = build_items()?;
                                    items = new_items;
                                    list_state.select(sel);
                                    status = Some(StatusMessage::Info(format!(
                                        "Mapped \"{}\" -> \"{name}\".",
                                        path.display()
                                    )));
                                }
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        }
                    }
                }
                KeyCode::Char('u') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        match crate::usage::get_profile_usage(&name, true) {
                            Ok(_) => {
                                let sel = list_state.selected();
                                let (new_items, _, _) = build_items()?;
                                items = new_items;
                                list_state.select(sel);
                                status = Some(StatusMessage::Info(format!(
                                    "Refreshed quota for \"{name}\"."
                                )));
                            }
                            Err(e) => {
                                status = Some(StatusMessage::Error(format!("Usage error: {e}")));
                            }
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        mode = Mode::ConfirmDelete { name };
                        status = None;
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        mode = Mode::Naming {
                            action: Action::Rename { old: name.clone() },
                            buffer: name,
                        };
                        status = None;
                    }
                }
                KeyCode::Char('e') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        let is_disabled = profiles::is_profile_disabled(&name).unwrap_or(false);
                        match profiles::set_profile_disabled(&name, !is_disabled) {
                            Ok(()) => {
                                let sel = list_state.selected();
                                let (new_items, new_current, _) = build_items()?;
                                items = new_items;
                                current_profile = new_current;
                                list_state.select(sel);
                                let state_str = if !is_disabled { "disabled" } else { "enabled" };
                                status = Some(StatusMessage::Info(format!(
                                    "Profile \"{name}\" is now {state_str}."
                                )));
                            }
                            Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                        }
                    }
                }
                KeyCode::Enter => {
                    let selected = &items[list_state.selected().unwrap_or(0)];
                    match selected {
                        Item::NewProfile => {
                            mode = Mode::Naming { action: Action::New, buffer: String::new() }
                        }
                        Item::ImportDefault => {
                            mode = Mode::Naming { action: Action::Import, buffer: String::new() }
                        }
                        Item::Profile { name, .. } => return Ok(Some(PickResult::Existing(name.clone()))),
                    }
                    status = None;
                }
                _ => {}
            },
            Mode::Naming { action, buffer } => match key.code {
                KeyCode::Esc => {
                    mode = Mode::Picking;
                    status = None;
                }
                KeyCode::Enter => {
                    let text = buffer.trim().to_string();
                    match action {
                        Action::Alias { profile } => {
                            let profile = profile.clone();
                            if text.is_empty() {
                                let _ = aliases::remove_aliases_for_profile(&profile);
                                let (new_items, new_current, _) = build_items()?;
                                items = new_items;
                                current_profile = new_current;
                                mode = Mode::Picking;
                                status = Some(StatusMessage::Info(format!(
                                    "Cleared aliases for \"{profile}\"."
                                )));
                            } else {
                                match aliases::set_alias_quiet(&profile, &text) {
                                    Ok(()) => {
                                        let (new_items, new_current, _) = build_items()?;
                                        items = new_items;
                                        current_profile = new_current;
                                        mode = Mode::Picking;
                                        status = Some(StatusMessage::Info(format!(
                                            "Aliased \"{text}\" -> \"{profile}\"."
                                        )));
                                    }
                                    Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                                }
                            }
                        }
                        Action::Rename { old } => {
                            let old = old.clone();
                            if text.is_empty() {
                                status = Some(StatusMessage::Error("profile name cannot be empty".to_string()));
                            } else if text == old {
                                mode = Mode::Picking;
                                status = None;
                            } else {
                                match profiles::rename_profile(&old, &text) {
                                    Ok(()) => {
                                        let (new_items, new_current, new_idx) = build_items()?;
                                        items = new_items;
                                        current_profile = new_current;
                                        list_state.select(Some(new_idx.unwrap_or(0)));
                                        mode = Mode::Picking;
                                        status = Some(StatusMessage::Info(format!(
                                            "Renamed profile to \"{text}\"."
                                        )));
                                    }
                                    Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                                }
                            }
                        }
                        Action::New => {
                            match profiles::validate_profile_name(&text) {
                                Ok(()) => return Ok(Some(PickResult::New(text))),
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        }
                        Action::Import => {
                            match profiles::validate_profile_name(&text) {
                                Ok(()) => return Ok(Some(PickResult::Import(text))),
                                Err(e) => status = Some(StatusMessage::Error(e.to_string())),
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c) => buffer.push(c),
                _ => {}
            },
            Mode::ConfirmDelete { name } => {
                let name = name.clone();
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => match profiles::remove_profile(&name) {
                        Ok(()) => {
                            let (new_items, new_current, _) = build_items()?;
                            items = new_items;
                            current_profile = new_current;
                            let idx = list_state
                                .selected()
                                .unwrap_or(0)
                                .min(items.len().saturating_sub(1));
                            list_state.select(Some(idx));
                            mode = Mode::Picking;
                            status = Some(StatusMessage::Info(format!("Deleted profile \"{name}\".")));
                        }
                        Err(e) => {
                            mode = Mode::Picking;
                            status = Some(StatusMessage::Error(e.to_string()));
                        }
                    },
                    _ => {
                        mode = Mode::Picking;
                        status = None;
                    }
                }
            }
        }
    }
}

fn draw(
    f: &mut Frame,
    items: &[Item],
    state: &mut ListState,
    mode: &Mode,
    status: &Option<StatusMessage>,
    current_profile: &Option<String>,
) {
    let accent_color = Color::Rgb(217, 119, 87);
    let border_color = Color::Rgb(71, 85, 105);
    let active_color = Color::Rgb(74, 222, 128);
    let muted_text = Color::Rgb(148, 163, 184);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Header line
            Constraint::Min(8),    // Main Two-Pane Area
            Constraint::Length(4), // Bottom Status & Keybindings
        ])
        .split(f.area());

    // 1. Top Header Line (clean, minimalist, informative)
    let header_line = Line::from(vec![
        Span::styled(" ✦ CLAUDE-USER ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(muted_text)),
        Span::styled("  ─  Multi-Account & Quota Manager", Style::default().fg(Color::Rgb(100, 116, 139))),
        match current_profile {
            Some(curr) => Span::styled(
                format!("    (active: {curr})"),
                Style::default().fg(active_color).add_modifier(Modifier::BOLD),
            ),
            None => Span::raw(""),
        },
    ]);
    f.render_widget(Paragraph::new(header_line), chunks[0]);

    // 2. Responsive Main Area Layout (Two side-by-side columns if width >= 80, stacked if narrow)
    let is_wide = f.area().width >= 80;
    let main_chunks = if is_wide {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(42),
                Constraint::Percentage(58),
            ])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ])
            .split(chunks[1])
    };

    // Left Column: ACCOUNTS list
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = state.selected() == Some(idx);
            let prefix = if is_selected { " ▶ " } else { "   " };

            match item {
                Item::Profile {
                    name,
                    alias,
                    is_current,
                    is_disabled,
                    is_mapped,
                    usage,
                    ..
                } => {
                    let style = if is_selected {
                        Style::default().fg(accent_color).add_modifier(Modifier::BOLD)
                    } else if *is_disabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let mut spans = vec![
                        Span::styled(prefix, Style::default().fg(accent_color)),
                        Span::styled(name.clone(), style),
                    ];

                    if let Some(a) = alias {
                        spans.push(Span::styled(format!(" (@{a})"), Style::default().fg(Color::Yellow)));
                    }

                    if *is_current {
                        spans.push(Span::styled(
                            " ● active",
                            Style::default().fg(active_color).add_modifier(Modifier::BOLD),
                        ));
                    }

                    if *is_mapped {
                        spans.push(Span::styled(
                            " ★ mapped",
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        ));
                    }

                    if *is_disabled {
                        spans.push(Span::styled(
                            " [disabled]",
                            Style::default().fg(Color::DarkGray),
                        ));
                    }

                    // Mini badge in row
                    if let Some(u) = usage {
                        match u.status {
                            crate::usage::UsageStatus::Ok => {
                                if let Some(h5) = &u.five_hour {
                                    let col = crate::usage::progress_color_ratatui(h5.pct);
                                    spans.push(Span::styled(
                                        format!("  {:.0}%", h5.pct),
                                        Style::default().fg(col).add_modifier(Modifier::BOLD),
                                    ));
                                }
                            }
                            crate::usage::UsageStatus::RateLimited => {
                                spans.push(Span::styled(" [429]", Style::default().fg(Color::Yellow)));
                            }
                            crate::usage::UsageStatus::TokenExpired => {
                                spans.push(Span::styled(" [expired]", Style::default().fg(Color::Red)));
                            }
                            _ => {}
                        }
                    }

                    ListItem::new(Line::from(spans))
                }
                Item::ImportDefault => {
                    let style = if is_selected {
                        Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(253, 224, 71))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(250, 204, 21))),
                        Span::styled(IMPORT_DEFAULT, style),
                    ]))
                }
                Item::NewProfile => {
                    let style = if is_selected {
                        Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(134, 239, 172))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(74, 222, 128))),
                        Span::styled(NEW_PROFILE, style),
                    ]))
                }
            }
        })
        .collect();

    let profiles_title = match current_profile {
        Some(curr) => format!(" PROFILES (active: {curr}) "),
        None => " PROFILES ".to_string(),
    };

    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                profiles_title,
                Style::default().fg(accent_color).add_modifier(Modifier::BOLD),
            )),
    );
    f.render_stateful_widget(list, main_chunks[0], state);

    // Right Column: LIVE QUOTA & RATE LIMITS
    let bar_width = (main_chunks[1].width as usize).saturating_sub(26).clamp(8, 22);
    let right_widget = match items.get(state.selected().unwrap_or(0)) {
        Some(Item::Profile {
            name,
            alias,
            email,
            org_name,
            is_current,
            is_disabled,
            is_mapped,
            usage,
        }) => {
            let mut lines = Vec::new();

            // Account info
            let mut name_spans = vec![
                Span::styled("Profile: ", Style::default().fg(muted_text)),
                Span::styled(name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ];
            if let Some(a) = alias {
                name_spans.push(Span::styled(format!("  (@{a})"), Style::default().fg(Color::Yellow)));
            }
            if *is_current {
                name_spans.push(Span::styled("  ● Currently Active", Style::default().fg(active_color).add_modifier(Modifier::BOLD)));
            }
            lines.push(Line::from(name_spans));

            let acc_str = match (email, org_name) {
                (Some(e), Some(o)) => format!("{e}  •  {o}"),
                (Some(e), None) => e.clone(),
                (None, Some(o)) => o.clone(),
                (None, None) => "No email/org metadata cached".to_string(),
            };
            lines.push(Line::from(vec![
                Span::styled("Account: ", Style::default().fg(muted_text)),
                Span::styled(acc_str, Style::default().fg(Color::Rgb(226, 232, 240))),
            ]));

            if *is_mapped {
                lines.push(Line::from(vec![
                    Span::styled("Directory: ", Style::default().fg(muted_text)),
                    Span::styled("Mapped to current working directory", Style::default().fg(Color::Cyan)),
                ]));
            }
            if *is_disabled {
                lines.push(Line::from(vec![
                    Span::styled("State: ", Style::default().fg(muted_text)),
                    Span::styled("Disabled (excluded from rotation)", Style::default().fg(Color::DarkGray)),
                ]));
            }

            lines.push(Line::raw(""));

            // Quota & Rate Limits
            lines.push(Line::from(vec![
                Span::styled("QUOTA & RATE LIMITS", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
            ]));

            match usage.as_deref() {
                Some(u) => {
                    let has_limits = u.five_hour.is_some() || u.seven_day.is_some() || u.spend.is_some() || !u.models.is_empty();
                    if let Some(h5) = &u.five_hour {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("5-Hour Limit: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(h5.pct, bar_width));
                        if let Some(rst) = &h5.countdown {
                            b_spans.push(Span::styled(format!("  (resets in {rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                        }
                        lines.push(Line::from(b_spans));
                    }

                    if let Some(d7) = &u.seven_day {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("7-Day Limit: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(d7.pct, bar_width));
                        if let Some(rst) = &d7.countdown {
                            b_spans.push(Span::styled(format!("  (resets in {rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                        }
                        lines.push(Line::from(b_spans));
                    }

                    if let Some(sp) = &u.spend {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("Extra Spend: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        let mut b_spans = vec![Span::raw("  ")];
                        b_spans.extend(crate::usage::render_tui_progress_spans(sp.pct, bar_width));
                        b_spans.push(Span::styled(
                            format!("  (${:.2} used of ${:.2} limit)", sp.used, sp.limit),
                            Style::default().fg(muted_text),
                        ));
                        lines.push(Line::from(b_spans));
                    }

                    if !u.models.is_empty() {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(vec![
                            Span::styled("Per-Model Weekly Limits:", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]));
                        for m in &u.models {
                            let mut m_spans = vec![Span::styled(format!("  {:<10} ", m.name), Style::default().fg(Color::Rgb(203, 213, 225)))];
                            m_spans.extend(crate::usage::render_tui_progress_spans(m.pct, bar_width.saturating_sub(4).max(6)));
                            if let Some(rst) = &m.countdown {
                                m_spans.push(Span::styled(format!(" ({rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                            }
                            lines.push(Line::from(m_spans));
                        }
                    }

                    // Status notes
                    match u.status {
                        crate::usage::UsageStatus::Ok => {
                            if !has_limits {
                                lines.push(Line::raw(""));
                                lines.push(Line::from(vec![
                                    Span::styled("  ● Account active. Usage API reports unlimited / no active restrictions.", Style::default().fg(active_color)),
                                ]));
                            }
                        }
                        crate::usage::UsageStatus::RateLimited => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ⚠ Anthropic usage endpoint is rate-limited (HTTP 429).", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                            ]));
                            lines.push(Line::from(vec![
                                Span::styled("    Press 'u' to refresh quota when limit window resets.", Style::default().fg(muted_text)),
                            ]));
                        }
                        crate::usage::UsageStatus::TokenExpired => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ⚠ OAuth access token is expired.", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                            ]));
                            lines.push(Line::from(vec![
                                Span::styled("    Press Enter to launch Claude Code and re-authenticate in browser.", Style::default().fg(Color::White)),
                            ]));
                        }
                        crate::usage::UsageStatus::NoUsageAccess => {
                            lines.push(Line::raw(""));
                            lines.push(Line::from(vec![
                                Span::styled("  ℹ OAuth rate limits API not enabled on this account tier.", Style::default().fg(Color::DarkGray)),
                            ]));
                        }
                        crate::usage::UsageStatus::Unavailable => {
                            if !has_limits {
                                lines.push(Line::raw(""));
                                let msg = u.error_message.as_deref().unwrap_or("No cached usage readings yet.");
                                lines.push(Line::from(vec![
                                    Span::styled(format!("  ℹ {msg}"), Style::default().fg(Color::DarkGray)),
                                ]));
                                lines.push(Line::from(vec![
                                    Span::styled("    Press 'u' to fetch live quota from Anthropic.", Style::default().fg(muted_text)),
                                ]));
                            }
                        }
                    }
                }
                None => {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![
                        Span::styled("  No cached readings. Press 'u' to fetch live quota.", Style::default().fg(muted_text)),
                    ]));
                }
            }

            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(" QUOTA & RATE LIMITS ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)))
            )
        }
        Some(Item::NewProfile) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled("ADD NEW ACCOUNT", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" (or press "),
                    Span::styled("n", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)),
                    Span::raw(") to create a new profile."),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("How it works:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  1. Choose a clean profile name (e.g. \"work\", \"personal\")."),
                ]),
                Line::from(vec![
                    Span::raw("  2. Claude Code opens and prompts browser OAuth sign-in."),
                ]),
                Line::from(vec![
                    Span::raw("  3. Credentials and rate limits stay completely isolated."),
                ]),
            ];
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(74, 222, 128)))
                    .title(Span::styled(" ADD ACCOUNT ", Style::default().fg(Color::Rgb(74, 222, 128)).add_modifier(Modifier::BOLD)))
            )
        }
        Some(Item::ImportDefault) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled("IMPORT EXISTING ACCOUNT", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" (or press "),
                    Span::styled("i", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)),
                    Span::raw(") to import existing ~/.claude login."),
                ]),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("How it works:", Style::default().fg(muted_text).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::raw("  Copies your active ~/.claude and ~/.claude.json into an isolated"),
                ]),
                Line::from(vec![
                    Span::raw("  profile so you can switch between accounts seamlessly."),
                ]),
            ];
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(250, 204, 21)))
                    .title(Span::styled(" IMPORT ACCOUNT ", Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)))
            )
        }
        None => {
            Paragraph::new(vec![Line::raw("No profile selected.")])
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)))
        }
    };
    f.render_widget(right_widget, main_chunks[1]);

    // 3. Bottom Status & Keybindings Area
    let status_line = match mode {
        Mode::Picking => {
            match status {
                Some(StatusMessage::Error(e)) => Line::from(vec![
                    Span::styled("✖ Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled(e.clone(), Style::default().fg(Color::Red)),
                ]),
                Some(StatusMessage::Info(msg)) => Line::from(vec![
                    Span::styled("✔ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(msg.clone(), Style::default().fg(Color::Green)),
                ]),
                None => Line::from(vec![
                    Span::styled("Tip: ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
                    Span::styled("Use ↑/↓ to browse live quota. Press ", Style::default().fg(muted_text)),
                    Span::styled("Enter", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(" to launch. Press ", Style::default().fg(muted_text)),
                    Span::styled("u", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(" to refresh quota.", Style::default().fg(muted_text)),
                ]),
            }
        }
        Mode::Naming { action, buffer } => {
            let label = match action {
                Action::New => "New Account / Profile Name",
                Action::Import => "Profile Name for Imported Account",
                Action::Rename { .. } => "New Name",
                Action::Alias { .. } => "Alias (blank to unset)",
            };
            match status {
                Some(StatusMessage::Error(e)) => Line::from(vec![
                    Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{buffer}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("   ({e})"), Style::default().fg(Color::Red)),
                ]),
                _ => Line::from(vec![
                    Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{buffer}_"), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled("   (Enter to confirm, Esc to cancel)", Style::default().fg(muted_text)),
                ]),
            }
        }
        Mode::ConfirmDelete { name } => {
            Line::from(vec![
                Span::styled(format!("Delete profile \"{name}\"? This removes its stored login. [y/N]"), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ])
        }
    };

    let keybindings_line = Line::from(vec![
        Span::styled(" [↑/↓] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Navigate   "),
        Span::styled(" [Enter] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Launch   "),
        Span::styled(" [s] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Switch   "),
        Span::styled(" [a] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Alias   "),
        Span::styled(" [m] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Map CWD   "),
        Span::styled(" [e] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Disable   "),
        Span::styled(" [r] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Rename   "),
        Span::styled(" [d] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Delete   "),
        Span::styled(" [u] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Refresh   "),
        Span::styled(" [q] ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)),
        Span::raw("Quit"),
    ]);

    let footer = Paragraph::new(vec![status_line, keybindings_line]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" STATUS & ACTIONS ", Style::default().fg(accent_color).add_modifier(Modifier::BOLD)))
    );
    f.render_widget(footer, chunks[2]);
}
