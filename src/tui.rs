use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::io::stdout;

use crate::profiles;

const NEW_PROFILE_LABEL: &str = "+ New profile";

enum Mode {
    Picking,
    NamingNew(String),
}

/// Runs the interactive picker. Returns the chosen profile name, or None if the
/// user quit without choosing one.
pub fn run_picker() -> Result<Option<String>> {
    let mut names = profiles::list_profiles()?;
    let mut items: Vec<String> = names.clone();
    items.push(NEW_PROFILE_LABEL.to_string());

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut mode = Mode::Picking;
    let mut error: Option<String> = None;

    let result = loop {
        terminal.draw(|f| draw(f, &items, &mut list_state, &mode, &error))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match &mut mode {
                Mode::Picking => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break None,
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
                        let i = list_state.selected().unwrap_or(0);
                        if items[i] == NEW_PROFILE_LABEL {
                            mode = Mode::NamingNew(String::new());
                            error = None;
                        } else {
                            break Some(items[i].clone());
                        }
                    }
                    _ => {}
                },
                Mode::NamingNew(buf) => match key.code {
                    KeyCode::Esc => {
                        mode = Mode::Picking;
                        error = None;
                    }
                    KeyCode::Enter => {
                        let name = buf.trim().to_string();
                        match profiles::validate_profile_name(&name) {
                            Ok(()) => break Some(name),
                            Err(e) => error = Some(e.to_string()),
                        }
                    }
                    KeyCode::Backspace => {
                        buf.pop();
                    }
                    KeyCode::Char(c) => {
                        buf.push(c);
                    }
                    _ => {}
                },
            }
        }
    };

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    names.clear();
    Ok(result)
}

fn draw(f: &mut Frame, items: &[String], state: &mut ListState, mode: &Mode, error: &Option<String>) {
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
        .map(|n| ListItem::new(Line::from(Span::raw(n.clone()))))
        .collect();
    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL).title("Profiles"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, chunks[1], state);

    let bottom_text = match mode {
        Mode::Picking => match error {
            Some(e) => format!("Error: {e}"),
            None => "Select a profile and press Enter.".to_string(),
        },
        Mode::NamingNew(buf) => match error {
            Some(e) => format!("New profile name: {buf}_   ({e})"),
            None => format!("New profile name: {buf}_   (Enter to confirm, Esc to cancel)"),
        },
    };
    let bottom = Paragraph::new(bottom_text).block(Block::default().borders(Borders::ALL));
    f.render_widget(bottom, chunks[2]);
}
