use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
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
}

enum Mode {
    Picking,
    Naming { action: Action, buffer: String },
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

fn event_loop(terminal: &mut CuserTerminal, items: Vec<Item>) -> Result<Option<PickResult>> {
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
                    match profiles::validate_profile_name(&name) {
                        Ok(()) => {
                            return Ok(Some(match action {
                                Action::New => PickResult::New(name),
                                Action::Import => PickResult::Import(name),
                            }));
                        }
                        Err(e) => error = Some(e.to_string()),
                    }
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c) => buffer.push(c),
                _ => {}
            },
        }
    }
}

fn draw(f: &mut Frame, items: &[Item], state: &mut ListState, mode: &Mode, error: &Option<String>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    let title = Paragraph::new("Claude account switcher — ↑/↓ move, Enter select, q quit")
        .block(Block::default().borders(Borders::ALL).title("cuser"));
    f.render_widget(title, chunks[0]);

    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            let text = match item {
                Item::Profile { display, .. } => display.as_str(),
                Item::ImportDefault => IMPORT_DEFAULT,
                Item::NewProfile => NEW_PROFILE,
            };
            ListItem::new(Line::from(Span::raw(text.to_string())))
        })
        .collect();
    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).title("Profiles"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, chunks[1], state);

    let bottom_text = match mode {
        Mode::Picking => error
            .clone()
            .map(|e| format!("Error: {e}"))
            .unwrap_or_else(|| "Select a profile and press Enter.".to_string()),
        Mode::Naming { buffer, .. } => match error {
            Some(e) => format!("Name: {buffer}_   ({e})"),
            None => format!("Name: {buffer}_   (Enter to confirm, Esc to cancel)"),
        },
    };
    let bottom = Paragraph::new(bottom_text).block(Block::default().borders(Borders::ALL));
    f.render_widget(bottom, chunks[2]);
}
