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

const NEW_PROFILE: &str = "+ New profile";
const IMPORT_DEFAULT: &str = "+ Import ~/.claude";

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
        display: String,
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

        let mut details = Vec::new();
        if !my_aliases.is_empty() {
            details.push(format!("@{}", my_aliases.join(", @")));
        }
        if let Some(email) = info.email {
            details.push(email);
        }
        if let Some(org) = info.org_name {
            details.push(org);
        }

        let display = if details.is_empty() {
            name.clone()
        } else {
            format!("{name}  ({})", details.join(" • "))
        };

        let usage_data = crate::usage::get_profile_usage(&name, false).ok().map(Box::new);
        items.push(Item::Profile {
            name,
            display,
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
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = list_state.selected().unwrap_or(0);
                    list_state.select(Some(i.saturating_sub(1)));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = list_state.selected().unwrap_or(0);
                    if i + 1 < items.len() {
                        list_state.select(Some(i + 1));
                    }
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
    let logo_color_1 = Color::Rgb(217, 119, 87);
    let logo_color_2 = Color::Rgb(148, 163, 184);
    let border_color = Color::Rgb(71, 85, 105);
    let active_color = Color::Rgb(74, 222, 128);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

    let logo = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  ░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀", Style::default().fg(logo_color_1)),
            Span::styled("░░░░░█░█░█▀▀░█▀▀░█▀▄", Style::default().fg(logo_color_2)),
        ]),
        Line::from(vec![
            Span::styled("  ░█░░░█░░░█▀█░█░█░█░█░█▀▀", Style::default().fg(logo_color_1)),
            Span::styled("░▄▄▄░█░█░▀▀█░█▀▀░█▀▄", Style::default().fg(logo_color_2)),
        ]),
        Line::from(vec![
            Span::styled("  ░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀", Style::default().fg(logo_color_1)),
            Span::styled("░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀", Style::default().fg(logo_color_2)),
        ]),
    ]);
    f.render_widget(logo, chunks[0]);

    let keybindings_info = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" ↑/↓ ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Navigate  |  "),
            Span::styled(" Enter ↵ ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Launch  |  "),
            Span::styled(" s ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Switch Active  |  "),
            Span::styled(" a ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Set Alias  |  "),
            Span::styled(" m ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Map CWD"),
        ]),
        Line::from(vec![
            Span::styled(" e ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Toggle Disabled  |  "),
            Span::styled(" r ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Rename  |  "),
            Span::styled(" d ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Delete  |  "),
            Span::styled(" u ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Refresh Quota  |  "),
            Span::styled(" q / Esc ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Quit"),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" KEYBINDINGS ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)))
    );
    f.render_widget(keybindings_info, chunks[1]);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = state.selected() == Some(idx);
            let prefix = if is_selected { " ▶ " } else { "   " };

            match item {
                Item::Profile { display, is_current, is_disabled, is_mapped, usage, .. } => {
                    let style = if is_selected {
                        Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)
                    } else if *is_disabled {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let mut spans = vec![
                        Span::styled(prefix, Style::default().fg(logo_color_1)),
                        Span::styled(display.clone(), style),
                    ];
                    if *is_current {
                        spans.push(Span::styled(
                            "  ● current",
                            Style::default().fg(active_color).add_modifier(Modifier::BOLD),
                        ));
                    }
                    if *is_mapped {
                        spans.push(Span::styled(
                            "  ★ mapped",
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ));
                    }
                    if let Some(u) = usage
                        && u.status == crate::usage::UsageStatus::Ok
                    {
                        if let Some(h5) = &u.five_hour {
                            spans.push(Span::styled("  5h ", Style::default().fg(Color::Rgb(148, 163, 184))));
                            spans.extend(crate::usage::render_tui_progress_spans(h5.pct, 6));
                        }
                        if let Some(d7) = &u.seven_day {
                            spans.push(Span::styled("  7d ", Style::default().fg(Color::Rgb(148, 163, 184))));
                            spans.extend(crate::usage::render_tui_progress_spans(d7.pct, 6));
                        }
                    }
                    if *is_disabled {
                        spans.push(Span::styled(
                            "  [disabled]",
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                }
                Item::ImportDefault => {
                    let style = if is_selected {
                        Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(logo_color_1)),
                        Span::styled(IMPORT_DEFAULT, style),
                    ]))
                }
                Item::NewProfile => {
                    let style = if is_selected {
                        Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(logo_color_1)),
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

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(profiles_title, Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)))
        );
    f.render_stateful_widget(list, chunks[2], state);

    let bottom_spans: Vec<Span<'static>> = match mode {
        Mode::Picking => {
            match status {
                Some(StatusMessage::Error(e)) => vec![Span::styled(format!("Error: {e}"), Style::default().fg(Color::Red))],
                Some(StatusMessage::Info(msg)) => vec![Span::styled(msg.clone(), Style::default().fg(Color::Green))],
                None => {
                    if let Some(Item::Profile { usage: Some(u), .. }) = items.get(state.selected().unwrap_or(0)) {
                        match u.status {
                            crate::usage::UsageStatus::Ok => {
                                let mut spans = vec![Span::styled("Quota: ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD))];
                                if let Some(h5) = &u.five_hour {
                                    spans.push(Span::styled("5h Limit ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                                    spans.extend(crate::usage::render_tui_progress_spans(h5.pct, 12));
                                    if let Some(rst) = &h5.countdown {
                                        spans.push(Span::styled(format!(" ({rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                                    }
                                }
                                if let Some(d7) = &u.seven_day {
                                    if u.five_hour.is_some() {
                                        spans.push(Span::styled("   •   ", Style::default().fg(border_color)));
                                    }
                                    spans.push(Span::styled("7d Limit ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
                                    spans.extend(crate::usage::render_tui_progress_spans(d7.pct, 12));
                                    if let Some(rst) = &d7.countdown {
                                        spans.push(Span::styled(format!(" ({rst})"), Style::default().fg(Color::Rgb(56, 189, 248))));
                                    }
                                }
                                spans
                            }
                            crate::usage::UsageStatus::TokenExpired => {
                                vec![Span::styled("Quota: OAuth token expired (select & launch to re-authenticate)", Style::default().fg(Color::Yellow))]
                            }
                            crate::usage::UsageStatus::RateLimited => {
                                vec![Span::styled("Quota: Rate-limited on Anthropic usage endpoint (429)", Style::default().fg(Color::Yellow))]
                            }
                            crate::usage::UsageStatus::NoUsageAccess => {
                                vec![Span::styled("Quota: Account tier does not report OAuth usage quota", Style::default().fg(Color::DarkGray))]
                            }
                            crate::usage::UsageStatus::Unavailable => {
                                match current_profile {
                                    Some(curr) => vec![Span::raw(format!("Select profile & press Enter. Running `claude` uses: \"{curr}\""))],
                                    None => vec![Span::raw("Select a profile and press Enter.")],
                                }
                            }
                        }
                    } else {
                        match current_profile {
                            Some(curr) => vec![Span::raw(format!("Select profile & press Enter. Running `claude` uses: \"{curr}\""))],
                            None => vec![Span::raw("Select a profile and press Enter.")],
                        }
                    }
                }
            }
        }
        Mode::Naming { action, buffer } => {
            let label = match action {
                Action::Rename { .. } => "New name",
                Action::Alias { .. } => "Alias (blank to unset)",
                _ => "Name",
            };
            match status {
                Some(StatusMessage::Error(e)) => vec![Span::styled(format!("{label}: {buffer}_   ({e})"), Style::default().fg(Color::Red))],
                _ => vec![Span::styled(format!("{label}: {buffer}_   (Enter to confirm, Esc to cancel)"), Style::default().fg(Color::White))],
            }
        }
        Mode::ConfirmDelete { name } => {
            vec![Span::styled(format!("Delete profile \"{name}\"? This removes its stored login. [y/N]"), Style::default().fg(Color::Yellow))]
        }
    };

    let bottom = Paragraph::new(Line::from(bottom_spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" STATUS ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)))
        );
    f.render_widget(bottom, chunks[3]);
}
