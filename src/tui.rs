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
}

enum Mode {
    Picking,
    Naming { action: Action, buffer: String },
    ConfirmDelete { name: String },
}

enum Item {
    Profile { name: String, display: String },
    ImportDefault,
    NewProfile,
}

pub fn run_picker() -> Result<Option<PickResult>> {
    let items = build_items()?;

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal: CuserTerminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, items);

    disable_raw_mode().ok();
    stdout().execute(LeaveAlternateScreen).ok();

    result
}

fn build_items() -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for name in profiles::list_profiles()? {
        let info = profiles::get_profile_info(&name).unwrap_or(profiles::ProfileInfo {
            name: name.clone(),
            email: None,
            org_name: None,
        });
        let display = match (info.email, info.org_name) {
            (Some(email), Some(org)) => format!("{name}  ({email} • {org})"),
            (Some(email), None) => format!("{name}  ({email})"),
            (None, _) => name.clone(),
        };
        items.push(Item::Profile { name, display });
    }
    if profiles::can_import()? {
        items.push(Item::ImportDefault);
    }
    items.push(Item::NewProfile);
    Ok(items)
}

fn selected_profile_name(items: &[Item], state: &ListState) -> Option<String> {
    match items.get(state.selected().unwrap_or(0)) {
        Some(Item::Profile { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

fn event_loop(terminal: &mut CuserTerminal, mut items: Vec<Item>) -> Result<Option<PickResult>> {
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut mode = Mode::Picking;
    let mut error: Option<String> = None;

    loop {
        terminal.draw(|f| draw(f, &items, &mut list_state, &mode, &error))?;

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
                KeyCode::Char('d') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        mode = Mode::ConfirmDelete { name };
                        error = None;
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(name) = selected_profile_name(&items, &list_state) {
                        mode = Mode::Naming {
                            action: Action::Rename { old: name.clone() },
                            buffer: name,
                        };
                        error = None;
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
                    error = None;
                }
                _ => {}
            },
            Mode::Naming { action, buffer } => match key.code {
                KeyCode::Esc => {
                    mode = Mode::Picking;
                    error = None;
                }
                KeyCode::Enter => {
                    let name = buffer.trim().to_string();
                    if let Action::Rename { old } = action {
                        let old = old.clone();
                        if name.is_empty() {
                            error = Some("profile name cannot be empty".to_string());
                        } else if name == old {
                            mode = Mode::Picking;
                            error = None;
                        } else {
                            match profiles::rename_profile(&old, &name) {
                                Ok(()) => {
                                    items = build_items()?;
                                    list_state.select(Some(0));
                                    mode = Mode::Picking;
                                    error = None;
                                }
                                Err(e) => error = Some(e.to_string()),
                            }
                        }
                    } else {
                        match profiles::validate_profile_name(&name) {
                            Ok(()) => {
                                return Ok(Some(match action {
                                    Action::New => PickResult::New(name),
                                    Action::Import => PickResult::Import(name),
                                    Action::Rename { .. } => unreachable!(),
                                }));
                            }
                            Err(e) => error = Some(e.to_string()),
                        }
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c) => buffer.push(c),
                _ => {}
            },
            Mode::ConfirmDelete { name } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => match profiles::remove_profile(name) {
                    Ok(()) => {
                        items = build_items()?;
                        let idx = list_state
                            .selected()
                            .unwrap_or(0)
                            .min(items.len().saturating_sub(1));
                        list_state.select(Some(idx));
                        mode = Mode::Picking;
                        error = None;
                    }
                    Err(e) => {
                        mode = Mode::Picking;
                        error = Some(e.to_string());
                    }
                },
                _ => {
                    mode = Mode::Picking;
                    error = None;
                }
            },
        }
    }
}

fn draw(f: &mut Frame, items: &[Item], state: &mut ListState, mode: &Mode, error: &Option<String>) {
    let logo_color_1 = Color::Rgb(217, 119, 87);
    let logo_color_2 = Color::Rgb(148, 163, 184);
    let border_color = Color::Rgb(71, 85, 105);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
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
            Span::raw("Select & Launch  |  "),
            Span::styled(" r ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Rename  |  "),
            Span::styled(" d ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Delete  |  "),
            Span::styled(" q / Esc ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)),
            Span::raw("Quit"),
        ])
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
            let text = match item {
                Item::Profile { display, .. } => display.as_str(),
                Item::ImportDefault => IMPORT_DEFAULT,
                Item::NewProfile => NEW_PROFILE,
            };
            let is_selected = state.selected() == Some(idx);
            let prefix = if is_selected { " ▶ " } else { "   " };
            let style = if is_selected {
                Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(logo_color_1)),
                Span::styled(text.to_string(), style),
            ]))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" PROFILES ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)))
        );
    f.render_stateful_widget(list, chunks[2], state);

    let bottom_text = match mode {
        Mode::Picking => error
            .clone()
            .map(|e| format!("Error: {e}"))
            .unwrap_or_else(|| "Select a profile and press Enter.".to_string()),
        Mode::Naming { action, buffer } => {
            let label = match action {
                Action::Rename { .. } => "New name",
                _ => "Name",
            };
            match error {
                Some(e) => format!("{label}: {buffer}_   ({e})"),
                None => format!("{label}: {buffer}_   (Enter to confirm, Esc to cancel)"),
            }
        }
        Mode::ConfirmDelete { name } => {
            format!("Delete profile \"{name}\"? This removes its stored login. [y/N]")
        }
    };
    
    let bottom_style = if error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    let bottom = Paragraph::new(Line::from(Span::styled(bottom_text, bottom_style)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" STATUS ", Style::default().fg(logo_color_1).add_modifier(Modifier::BOLD)))
        );
    f.render_widget(bottom, chunks[3]);
}
